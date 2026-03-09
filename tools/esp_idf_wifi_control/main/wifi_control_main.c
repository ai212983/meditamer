#include <stdbool.h>
#include <inttypes.h>
#include <string.h>

#include "freertos/FreeRTOS.h"
#include "freertos/event_groups.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_system.h"
#include "esp_wifi.h"
#include "nvs_flash.h"

#define WIFI_CONNECTED_BIT BIT0
#define WIFI_FAIL_BIT BIT1

static const char *TAG = "wifi_control";
static EventGroupHandle_t s_wifi_event_group;
static int s_retry_num;

static bool wifi_control_connect_mode(void)
{
    return strlen(CONFIG_WIFI_CONTROL_SSID) > 0;
}

static wifi_auth_mode_t wifi_control_auth_threshold(void)
{
    return strlen(CONFIG_WIFI_CONTROL_PASSWORD) > 0 ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
}

static void wifi_control_log_ap(const wifi_ap_record_t *ap, int idx)
{
    ESP_LOGI(
        TAG,
        "scan_ap idx=%d ssid=%s rssi=%d channel=%d auth=%d bssid=%02x:%02x:%02x:%02x:%02x:%02x",
        idx,
        ap->ssid,
        ap->rssi,
        ap->primary,
        ap->authmode,
        ap->bssid[0],
        ap->bssid[1],
        ap->bssid[2],
        ap->bssid[3],
        ap->bssid[4],
        ap->bssid[5]
    );
}

static char wifi_control_cc_char(uint8_t byte)
{
    return (byte >= 0x21 && byte <= 0x7e) ? (char)byte : '.';
}

static void wifi_control_log_driver_state(const char *label)
{
    wifi_mode_t mode = WIFI_MODE_NULL;
    wifi_ps_type_t ps = WIFI_PS_NONE;
    uint8_t protocol_bitmap = 0;
    uint8_t primary = 0;
    wifi_second_chan_t second = WIFI_SECOND_CHAN_NONE;
    int8_t max_tx_power = 0;
    uint32_t event_mask = 0;
    wifi_scan_default_params_t scan_defaults = { 0 };
    wifi_country_t country = { 0 };
    esp_err_t mode_rc = esp_wifi_get_mode(&mode);
    esp_err_t channel_rc = esp_wifi_get_channel(&primary, &second);
    esp_err_t ps_rc = esp_wifi_get_ps(&ps);
    esp_err_t max_tx_power_rc = esp_wifi_get_max_tx_power(&max_tx_power);
    esp_err_t event_mask_rc = esp_wifi_get_event_mask(&event_mask);
    esp_err_t protocol_rc = esp_wifi_get_protocol(WIFI_IF_STA, &protocol_bitmap);
    esp_err_t scan_defaults_rc = esp_wifi_get_scan_parameters(&scan_defaults);
    esp_err_t country_rc = esp_wifi_get_country(&country);

    ESP_LOGI(
        TAG,
        "%s mode_rc=%d mode=%d channel_rc=%d primary=%u second=%d ps_rc=%d ps=%d max_tx_power_rc=%d max_tx_power=%d event_mask_rc=%d event_mask=0x%08" PRIx32 " protocol_rc=%d protocol_bitmap=0x%02x country_rc=%d cc=%c%c%c schan=%u nchan=%u country_max_tx_power=%d policy=%d scan_defaults_rc=%d scan_active_min=%u scan_active_max=%u scan_passive=%u scan_home_dwell=%u",
        label,
        mode_rc,
        mode,
        channel_rc,
        primary,
        second,
        ps_rc,
        ps,
        max_tx_power_rc,
        max_tx_power,
        event_mask_rc,
        event_mask,
        protocol_rc,
        protocol_bitmap,
        country_rc,
        wifi_control_cc_char(country.cc[0]),
        wifi_control_cc_char(country.cc[1]),
        wifi_control_cc_char(country.cc[2]),
        country.schan,
        country.nchan,
        country.max_tx_power,
        country.policy,
        scan_defaults_rc,
        scan_defaults.scan_time.active.min,
        scan_defaults.scan_time.active.max,
        scan_defaults.scan_time.passive,
        scan_defaults.home_chan_dwell_time
    );
}

static void wifi_control_event_handler(
    void *arg,
    esp_event_base_t event_base,
    int32_t event_id,
    void *event_data
)
{
    if (event_base == WIFI_EVENT && event_id == WIFI_EVENT_STA_START) {
        ESP_LOGI(TAG, "event sta_start");
        ESP_ERROR_CHECK(esp_wifi_connect());
        return;
    }

    if (event_base == WIFI_EVENT && event_id == WIFI_EVENT_STA_DISCONNECTED) {
        wifi_event_sta_disconnected_t *event = (wifi_event_sta_disconnected_t *)event_data;
        ESP_LOGW(TAG, "event sta_disconnected reason=%d retry=%d", event->reason, s_retry_num);
        if (s_retry_num < CONFIG_WIFI_CONTROL_MAXIMUM_RETRY) {
            ESP_ERROR_CHECK(esp_wifi_connect());
            s_retry_num++;
            return;
        }
        xEventGroupSetBits(s_wifi_event_group, WIFI_FAIL_BIT);
        return;
    }

    if (event_base == IP_EVENT && event_id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *event = (ip_event_got_ip_t *)event_data;
        ESP_LOGI(TAG, "event got_ip ip=" IPSTR, IP2STR(&event->ip_info.ip));
        s_retry_num = 0;
        xEventGroupSetBits(s_wifi_event_group, WIFI_CONNECTED_BIT);
    }
}

static void wifi_control_init_common(void)
{
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_t *sta_netif = esp_netif_create_default_wifi_sta();
    assert(sta_netif != NULL);

    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
#if CONFIG_WIFI_CONTROL_DISABLE_WIFI_NVS
    cfg.nvs_enable = 0;
#endif
    ESP_LOGI(TAG, "wifi_init nvs_enable=%d", cfg.nvs_enable);
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
#if CONFIG_WIFI_CONTROL_FORCE_PS_NONE
    ESP_ERROR_CHECK(esp_wifi_set_ps(WIFI_PS_NONE));
#endif
#if CONFIG_WIFI_CONTROL_FORCE_COUNTRY_CN_MANUAL
    wifi_country_t country = { .cc = { 'C', 'N', 0 }, .schan = 1, .nchan = 13, .policy = WIFI_COUNTRY_POLICY_MANUAL, .max_tx_power = 20 };
    ESP_ERROR_CHECK(esp_wifi_set_country(&country));
#endif
#if CONFIG_WIFI_CONTROL_FORCE_MAX_TX_POWER_80
    ESP_ERROR_CHECK(esp_wifi_set_max_tx_power(80));
#endif
}

static void wifi_control_run_scan_only(void)
{
    uint16_t requested = CONFIG_WIFI_CONTROL_SCAN_LIST_SIZE;
    uint16_t ap_count = 0;
    wifi_ap_record_t ap_info[CONFIG_WIFI_CONTROL_SCAN_LIST_SIZE];

    memset(ap_info, 0, sizeof(ap_info));

    ESP_LOGI(TAG, "mode=scan_only scan_list_size=%u", requested);
    ESP_ERROR_CHECK(esp_wifi_start());
    wifi_control_log_driver_state("pre_scan_driver_state");
    ESP_ERROR_CHECK(esp_wifi_scan_start(NULL, true));
    ESP_ERROR_CHECK(esp_wifi_scan_get_ap_num(&ap_count));
    ESP_ERROR_CHECK(esp_wifi_scan_get_ap_records(&requested, ap_info));

    ESP_LOGI(TAG, "scan_complete total_ap_count=%u returned_ap_count=%u", ap_count, requested);
    for (int i = 0; i < requested; i++) {
        wifi_control_log_ap(&ap_info[i], i);
    }
}

static void wifi_control_run_connect(void)
{
    wifi_config_t wifi_config = {
        .sta = {
            .threshold.authmode = WIFI_AUTH_OPEN,
        },
    };
    EventBits_t bits;
    esp_event_handler_instance_t wifi_handler;
    esp_event_handler_instance_t ip_handler;

    s_wifi_event_group = xEventGroupCreate();
    assert(s_wifi_event_group != NULL);

    strncpy((char *)wifi_config.sta.ssid, CONFIG_WIFI_CONTROL_SSID, sizeof(wifi_config.sta.ssid));
    strncpy(
        (char *)wifi_config.sta.password,
        CONFIG_WIFI_CONTROL_PASSWORD,
        sizeof(wifi_config.sta.password)
    );
    wifi_config.sta.threshold.authmode = wifi_control_auth_threshold();

    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        WIFI_EVENT,
        ESP_EVENT_ANY_ID,
        &wifi_control_event_handler,
        NULL,
        &wifi_handler
    ));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        IP_EVENT,
        IP_EVENT_STA_GOT_IP,
        &wifi_control_event_handler,
        NULL,
        &ip_handler
    ));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wifi_config));

    ESP_LOGI(
        TAG,
        "mode=connect ssid=%s threshold_auth=%d max_retry=%d wait_ms=%d",
        CONFIG_WIFI_CONTROL_SSID,
        wifi_config.sta.threshold.authmode,
        CONFIG_WIFI_CONTROL_MAXIMUM_RETRY,
        CONFIG_WIFI_CONTROL_CONNECT_WAIT_MS
    );
    ESP_ERROR_CHECK(esp_wifi_start());

    bits = xEventGroupWaitBits(
        s_wifi_event_group,
        WIFI_CONNECTED_BIT | WIFI_FAIL_BIT,
        pdFALSE,
        pdFALSE,
        pdMS_TO_TICKS(CONFIG_WIFI_CONTROL_CONNECT_WAIT_MS)
    );

    if (bits & WIFI_CONNECTED_BIT) {
        ESP_LOGI(TAG, "connect_result=connected ssid=%s", CONFIG_WIFI_CONTROL_SSID);
    } else if (bits & WIFI_FAIL_BIT) {
        ESP_LOGW(TAG, "connect_result=failed ssid=%s", CONFIG_WIFI_CONTROL_SSID);
    } else {
        ESP_LOGW(TAG, "connect_result=timeout ssid=%s", CONFIG_WIFI_CONTROL_SSID);
    }

    ESP_ERROR_CHECK(esp_event_handler_instance_unregister(
        WIFI_EVENT,
        ESP_EVENT_ANY_ID,
        wifi_handler
    ));
    ESP_ERROR_CHECK(esp_event_handler_instance_unregister(
        IP_EVENT,
        IP_EVENT_STA_GOT_IP,
        ip_handler
    ));
    vEventGroupDelete(s_wifi_event_group);
}

void app_main(void)
{
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ret = nvs_flash_init();
    }
    ESP_ERROR_CHECK(ret);

    wifi_control_init_common();

    if (wifi_control_connect_mode()) {
        wifi_control_run_connect();
    } else {
        wifi_control_run_scan_only();
    }
}
