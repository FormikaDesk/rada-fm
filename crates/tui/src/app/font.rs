//! The offer to install a Nerd Font, made once.

use super::*;

pub enum FontStage {
    Ask,
    Working,
}

pub struct FontView {
    pub offer: FontOffer,
    pub stage: FontStage,
}

impl App {
    /// Show the offer, if one is to be made and nothing else is in the way.
    pub(super) fn maybe_offer_font(&mut self, offer: Option<FontOffer>) {
        let Some(offer) = offer else { return };
        if self.modal.is_some() {
            return;
        }
        (offer.shown)();
        self.modal = Some(Modal::Font(FontView {
            offer,
            stage: FontStage::Ask,
        }));
    }

    pub(super) fn on_font_key(&mut self, mut view: FontView, key: KeyEvent) {
        match (&view.stage, key.code) {
            (FontStage::Ask, KeyCode::Enter | KeyCode::Char('y')) => {
                view.stage = FontStage::Working;
                let install = view.offer.install.clone();
                let notes = self.svc.notes.clone();
                std::thread::spawn(move || {
                    let _ = notes.send(CoreEvent::FontInstalled(install()));
                });
            }
            (FontStage::Ask, KeyCode::Esc | KeyCode::Char('n' | 'q')) => return,
            (FontStage::Working, KeyCode::Esc) => return,
            _ => {}
        }
        self.modal = Some(Modal::Font(view));
    }

    pub(super) fn on_font_installed(&mut self, result: Result<String, String>) {
        if matches!(self.modal, Some(Modal::Font(_))) {
            self.modal = None;
        }
        match result {
            Ok(text) => self.toast(ToastKind::Ok, text, 15),
            Err(e) => self.toast(ToastKind::Error, tf(Key::TFontNotInstalled, &[&e]), 10),
        }
    }
}
