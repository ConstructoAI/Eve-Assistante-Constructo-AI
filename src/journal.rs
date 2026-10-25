//! Le journal d'Eve : `%LOCALAPPDATA%\EntrepreneurGeneral\Eve\eve.log`, hors du dossier partagé. On y lit les demandes et
//! les réponses : rien de ce qui s'y écrit ne quitte le poste. Jamais un jeton ni une clé.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static FICHIER: Mutex<Option<File>> = Mutex::new(None);
/// Chaque ligne passe aussi à la console, sauf pour `eve --mettre-a-jour` et son gardien ([`sans_console`]).
static CONSOLE: AtomicBool = AtomicBool::new(true);

/// Le journal sans écho à la console : `eve --mettre-a-jour` dit lui-même, en une ligne, ce qui va se passer (sous
/// PowerShell, chaque ligne de plus sur la sortie d'erreur passerait pour une erreur).
pub fn sans_console() {
    CONSOLE.store(false, Ordering::Relaxed);
}

/// Ouvre le journal (le précédent, au-delà de 5 Mo, devient `eve.1.log`).
pub fn ouvrir(dossier: &Path) {
    let chemin = dossier.join("eve.log");
    if fs::metadata(&chemin).map(|m| m.len() > 5_000_000).unwrap_or(false) {
        let _ = fs::rename(&chemin, dossier.join("eve.1.log"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&chemin)
        && let Ok(mut g) = FICHIER.lock()
    {
        *g = Some(f);
    }
}

/// Le journal d'un dossier local, à relire : le précédent (`eve.1.log`, mis de côté au-delà de 5 Mo), puis l'actuel ;
/// rien de ce qui manque.
pub fn lire_tout(dossier: &Path) -> String {
    let lire = |nom: &str| fs::read(dossier.join(nom)).map(|o| String::from_utf8_lossy(&o).into_owned());
    format!("{}\n{}", lire("eve.1.log").unwrap_or_default(), lire("eve.log").unwrap_or_default())
}

/// Une ligne au journal (et à la console).
pub fn ecrire(message: &str) {
    let (a, mo, j, h, mi, s) = maintenant();
    let ligne = format!("{a:04}-{mo:02}-{j:02} {h:02}:{mi:02}:{s:02} {message}\n");
    if CONSOLE.load(Ordering::Relaxed) {
        eprint!("{ligne}");
    }
    if let Ok(mut g) = FICHIER.lock()
        && let Some(f) = g.as_mut()
    {
        let _ = f.write_all(ligne.as_bytes());
    }
}

/// La console du journal sans le mode d'édition rapide : sous Windows, un clic qui sélectionne du texte dans la
/// console fige le programme à sa prochaine écriture (la revue du 2026-10-21) ; sans ce mode, la console garde le
/// journal à lire sans jamais rien bloquer.
#[cfg(windows)]
pub fn proteger_la_console() {
    use windows::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_EXTENDED_FLAGS, ENABLE_QUICK_EDIT_MODE, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
        SetConsoleMode,
    };
    // SAFETY : le handle d'entrée de la console du processus, lu puis réglé ; sans console, les appels échouent sans
    // effet.
    unsafe {
        let Ok(entree) = GetStdHandle(STD_INPUT_HANDLE) else { return };
        let mut mode = CONSOLE_MODE::default();
        if GetConsoleMode(entree, &mut mode).is_ok() {
            let _ = SetConsoleMode(entree, (mode & !ENABLE_QUICK_EDIT_MODE) | ENABLE_EXTENDED_FLAGS);
        }
    }
}

#[cfg(not(windows))]
pub fn proteger_la_console() {}

/// L'heure locale, « 21:42 ».
pub fn heure() -> String {
    let (_, _, _, h, mi, _) = maintenant();
    format!("{h:02}:{mi:02}")
}

/// La date locale, « 2026-10-21 ».
pub fn date() -> String {
    let (a, mo, j, _, _, _) = maintenant();
    format!("{a:04}-{mo:02}-{j:02}")
}

/// La date et l'heure locales, à la seconde : « 2026-10-22 10:15:02 ».
pub fn date_et_heure() -> String {
    let (a, mo, j, h, mi, s) = maintenant();
    format!("{a:04}-{mo:02}-{j:02} {h:02}:{mi:02}:{s:02}")
}

/// La date et l'heure locales, à la seconde, bonnes pour un nom de fichier : « 2026-10-22 10-15-02 ».
pub fn horodatage() -> String {
    let (a, mo, j, h, mi, s) = maintenant();
    format!("{a:04}-{mo:02}-{j:02} {h:02}-{mi:02}-{s:02}")
}

/// La date et l'heure locales au format des PDF : « D:20261022101502 ».
pub fn date_pdf() -> String {
    let (a, mo, j, h, mi, s) = maintenant();
    format!("D:{a:04}{mo:02}{j:02}{h:02}{mi:02}{s:02}")
}

#[cfg(windows)]
fn maintenant() -> (u16, u16, u16, u16, u16, u16) {
    // SAFETY : GetLocalTime remplit une structure qu'il rend par valeur ; aucun pointeur ne survit à l'appel.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Ailleurs que sous Windows : l'heure UTC (le calendrier civil de Howard Hinnant).
#[cfg(not(windows))]
fn maintenant() -> (u16, u16, u16, u16, u16, u16) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (jours, reste) = ((s / 86_400) as i64, s % 86_400);
    let z = jours + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let a = yoe + ere * 400 + i64::from(mo <= 2);
    (a as u16, mo as u16, j as u16, (reste / 3600) as u16, (reste % 3600 / 60) as u16, (reste % 60) as u16)
}
