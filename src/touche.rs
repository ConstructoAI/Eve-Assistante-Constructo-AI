//! La touche à tenir pour parler, captée partout — la fenêtre d'Eve n'a pas besoin d'avoir le focus. Son état se
//! lit toutes les 15 ms (GetAsyncKeyState) : ni crochet clavier, ni droit d'administrateur. Elle change sans
//! redémarrer quand `touche` change dans reglages.json ([`changer`]).

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::chef::Evt;

/// La touche suivie : son code de touche virtuelle Windows.
static TOUCHE: AtomicI32 = AtomicI32::new(0);

pub fn demarrer(code: i32, rapport: Sender<Evt>) {
    changer(code);
    thread::spawn(move || {
        let mut avant = false;
        loop {
            let bas = est_enfoncee(TOUCHE.load(Ordering::Relaxed));
            if bas != avant {
                avant = bas;
                if rapport.send(Evt::Touche(bas)).is_err() {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
    });
}

/// La touche à suivre désormais (l'ancienne, tenue à cet instant, compte pour relâchée).
pub fn changer(code: i32) {
    TOUCHE.store(code, Ordering::Relaxed);
}

#[cfg(windows)]
fn est_enfoncee(code: i32) -> bool {
    // SAFETY : GetAsyncKeyState lit l'état d'une touche ; aucun pointeur.
    let etat = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(code) };
    (etat as u16) & 0x8000 != 0
}

#[cfg(not(windows))]
fn est_enfoncee(_code: i32) -> bool {
    false
}
