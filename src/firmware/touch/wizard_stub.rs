use crate::firmware::touch::types::TouchEvent;
use crate::firmware::types::InkplateDriver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WizardDispatch {
    Inactive,
    Consumed,
    Finished,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchCalibrationWizard {
    active: bool,
}

impl TouchCalibrationWizard {
    pub(crate) fn new(active: bool) -> Self {
        Self { active }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) async fn render_full(&mut self, _display: &mut InkplateDriver) {}

    pub(crate) async fn handle_event(
        &mut self,
        _display: &mut InkplateDriver,
        _event: TouchEvent,
    ) -> WizardDispatch {
        WizardDispatch::Inactive
    }
}

pub(crate) async fn render_touch_wizard_waiting_screen(_display: &mut InkplateDriver) {}
