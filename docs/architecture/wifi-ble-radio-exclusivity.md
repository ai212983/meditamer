# Wi-Fi/BLE radio exclusivity

## Problem

Wi-Fi and BLE share radio resources and consume scarce internal memory. Merely
stopping two separately allocated tasks does not release their reserved task
storage.

## Decision

Use mutually exclusive Wi-Fi and BLE service runs. One supervisor owns the radio
and constructs the requested service after the previous service has shut down.
It awaits the services sequentially, rather than keeping both service futures
resident together.

Shutdown must confirm that driver work has ended and callbacks can no longer
access the old service. An idle status alone is insufficient. The next service
must wait for that confirmation. If shutdown cannot establish this, refuse the
switch and require reboot recovery.

## Cost

Switching interrupts the previous service and takes time to shut it down and
restart it. Wi-Fi and BLE cannot serve consumers simultaneously.

Sequential execution is chosen to reduce task-storage demand; it does not itself
prove that a build fits memory or that teardown works on hardware. Supporting
simultaneous services would require a different resource design and validation.
