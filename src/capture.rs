//! « Regarde mon écran » : une capture de l'écran où est la souris, sans les fenêtres d'Eve — Windows les ôte de la
//! capture le temps de la prendre (`WDA_EXCLUDEFROMCAPTURE`), et l'on voit ce qu'elles cachaient —, mise en PNG et
//! jointe à la demande comme un fichier déposé ([`crate::ecran::joindre`]). Le 28 octobre 2026, Claude a dû répondre
//! « je ne vois pas ton écran » : la personne regardait un tableau de Stripe, et l'a recopié à la main. La capture ne
//! se prend qu'à la demande de la personne ([`eve::parole::demande_l_ecran`]) ; elle reste sur le poste, dans les
//! pièces jointes du jour, et part à Claude avec la demande, comme une capture collée.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Instant;

use crate::chef::Evt;

/// Une capture : sa largeur, sa hauteur, ses pixels (RGBA, rangée par rangée).
pub struct Image {
    pub largeur: u32,
    pub hauteur: u32,
    pub rgba: Vec<u8>,
}

/// Dans un fil : la capture de l'écran, jointe à la prochaine demande (« Écran 2026-10-28 13-05-00.png »), puis
/// [`Evt::EcranPris`] au chef, qui envoie avec elle la demande `texte` (dite, finie à `fin_de_parole` ; écrite : `None`).
/// La pièce arrive au chef avant lui : la même file, dans l'ordre.
pub fn prendre_et_joindre(texte: String, fin_de_parole: Option<Instant>, rapport: Sender<Evt>) {
    thread::spawn(move || {
        let nom = format!("Écran {}.png", crate::journal::horodatage());
        let png = imp::ecran_sous_la_souris().and_then(|i| crate::cartes::png_rgba(i.largeur, i.hauteur, &i.rgba));
        let erreur = match png {
            Ok(octets) => crate::ecran::joindre(&nom, octets.len() as u64, octets.as_slice(), &rapport)
                .err()
                .map(|r| r.to_string()),
            Err(e) => Some(e),
        };
        let _ = rapport.send(Evt::EcranPris { texte, fin_de_parole, erreur });
    });
}

#[cfg(windows)]
mod imp {
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
        MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };

    use super::Image;

    /// Le temps que Windows recompose l'écran sans les fenêtres ôtées de la capture (deux ou trois images à 60 Hz,
    /// largement).
    pub(super) const RECOMPOSER: Duration = Duration::from_millis(150);

    /// L'écran où est la souris, entier, sans les fenêtres de ce programme.
    pub(super) fn ecran_sous_la_souris() -> Result<Image, String> {
        let r = ecran_de_la_souris()?;
        let otees = oter(&crate::fenetre::fenetres_de_ce_programme());
        if !otees.is_empty() {
            thread::sleep(RECOMPOSER);
        }
        let image = copier(r.left, r.top, r.right - r.left, r.bottom - r.top);
        rendre(&otees);
        image
    }

    /// Le rectangle de l'écran où est la souris, en pixels de l'écran.
    pub(super) fn ecran_de_la_souris() -> Result<RECT, String> {
        let mut p = POINT::default();
        // SAFETY : Windows écrit la position de la souris dans `p`, qui vit pendant l'appel.
        unsafe { GetCursorPos(&raw mut p) }.map_err(|e| format!("la souris ne se trouve pas ({e})"))?;
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY : des lectures ; `info` vit pendant l'appel, sa taille est celle qu'on annonce.
        let lu = unsafe { GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &raw mut info) }.as_bool();
        if lu { Ok(info.rcMonitor) } else { Err("l'écran de la souris ne se décrit pas".into()) }
    }

    /// Ôte ces fenêtres des captures de l'écran (elles restent à l'écran) ; rend celles qui le sont.
    pub(super) fn oter(fenetres: &[HWND]) -> Vec<HWND> {
        fenetres
            .iter()
            .copied()
            // SAFETY : un réglage d'affichage d'une fenêtre de ce programme ; un identifiant périmé échoue, sans plus.
            .filter(|&f| unsafe { SetWindowDisplayAffinity(f, WDA_EXCLUDEFROMCAPTURE) }.is_ok())
            .collect()
    }

    /// Les remet dans les captures.
    pub(super) fn rendre(fenetres: &[HWND]) {
        for &f in fenetres {
            // SAFETY : comme [`oter`].
            let _ = unsafe { SetWindowDisplayAffinity(f, WDA_NONE) };
        }
    }

    /// Les pixels de l'écran dans ce rectangle (en pixels de l'écran), tels que Windows les compose, en RGBA opaque.
    pub(super) fn copier(x: i32, y: i32, l: i32, h: i32) -> Result<Image, String> {
        if l <= 0 || h <= 0 {
            return Err("un écran sans taille".into());
        }
        let mut pixels = vec![0u8; l as usize * h as usize * 4];
        // SAFETY : le contexte de l'écran, une copie en mémoire et son image, créés ici et rendus ou détruits avant de
        // sortir ; `GetDIBits` écrit au plus `h` rangées de `l` pixels de 32 bits — la taille de `pixels` — d'après
        // `bmi`, qui vit pendant l'appel (une hauteur négative : la première rangée en haut).
        let (copie, lignes) = unsafe {
            let ecran = GetDC(None);
            if ecran.is_invalid() {
                return Err("l'écran ne se lit pas".into());
            }
            let memoire = CreateCompatibleDC(Some(ecran));
            let image = CreateCompatibleBitmap(ecran, l, h);
            let avant = SelectObject(memoire, image.into());
            let copie = BitBlt(memoire, 0, 0, l, h, Some(ecran), x, y, SRCCOPY);
            SelectObject(memoire, avant);
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: l,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let lignes =
                GetDIBits(memoire, image, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &raw mut bmi, DIB_RGB_COLORS);
            let _ = DeleteObject(image.into());
            let _ = DeleteDC(memoire);
            ReleaseDC(None, ecran);
            (copie, lignes)
        };
        copie.map_err(|e| format!("l'écran ne se copie pas ({e})"))?;
        if lignes != h {
            return Err(format!("{lignes} rangées de l'écran lues sur {h}"));
        }
        // Windows les rend en BGRA, l'octet de transparence vide.
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        Ok(Image { largeur: l as u32, hauteur: h as u32, rgba: pixels })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Image;

    pub(super) fn ecran_sous_la_souris() -> Result<Image, String> {
        Err("la capture de l'écran ne se fait que sous Windows".into())
    }
}

/// `eve --essai-capture` : une petite fenêtre d'essai, unie, posée sous la souris (sans prendre le clavier), capturée
/// telle quelle, puis ôtée de la capture comme Eve ôte les siennes ; dit si elle a disparu de la seconde, et la taille
/// de l'écran que prendrait « regarde mon écran ». Rien ne s'enregistre : les pixels de l'écran ne quittent pas la
/// mémoire. Le code de sortie dit si la fenêtre a disparu.
#[cfg(windows)]
pub fn essai() -> i32 {
    use std::time::Duration;

    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetCursorPos, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
        WINDOW_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::w;

    /// SS_BLACKRECT : la classe STATIC peint son rectangle, uni, de la couleur du cadre des fenêtres.
    const RECTANGLE_NOIR: WINDOW_STYLE = WINDOW_STYLE(0x4);
    const L: i32 = 240;
    const H: i32 = 160;

    // Comme la fenêtre d'Eve (winit) : des pixels de l'écran, pas des points mis à l'échelle.
    // SAFETY : un réglage du processus, avant toute fenêtre.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let attendre = |duree: Duration| {
        let fin = Instant::now() + duree;
        while Instant::now() < fin {
            let mut m = MSG::default();
            // SAFETY : la file des messages de ce fil, `m` vit pendant les appels.
            while unsafe { PeekMessageW(&raw mut m, None, 0, 0, PM_REMOVE) }.as_bool() {
                // SAFETY : le message que Windows vient de remettre, à la fenêtre de ce fil.
                unsafe {
                    let _ = TranslateMessage(&raw const m);
                    DispatchMessageW(&raw const m);
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    };
    let mut p = POINT::default();
    // SAFETY : Windows écrit la position de la souris dans `p`, qui vit pendant l'appel.
    let _ = unsafe { GetCursorPos(&raw mut p) };
    let (x, y) = (p.x - L / 2, p.y - H / 2);
    // SAFETY : une fenêtre de la classe STATIC de Windows, à ce programme, détruite plus bas.
    let fenetre = match unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("STATIC"),
            w!(""),
            WS_POPUP | WS_VISIBLE | RECTANGLE_NOIR,
            x,
            y,
            L,
            H,
            None,
            None,
            None,
            None,
        )
    } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("la fenêtre d'essai ne s'ouvre pas : {e}");
            return 1;
        }
    };
    attendre(Duration::from_millis(400));
    let vue = imp::copier(x, y, L, H);
    let otees = imp::oter(&[fenetre]);
    attendre(imp::RECOMPOSER);
    let otee = imp::copier(x, y, L, H);
    imp::rendre(&otees);
    // SAFETY : la fenêtre créée plus haut.
    let _ = unsafe { DestroyWindow(fenetre) };
    let (vue, otee) = match (vue, otee) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("la capture ne se fait pas : {e}");
            return 1;
        }
    };
    // La part des pixels de la couleur du rectangle dans la première capture (la fenêtre s'est-elle vue ? Windows le peint
    // de la couleur du cadre des fenêtres, un gris foncé sous Windows 11), et celle des pixels changés dans la seconde
    // (a-t-elle disparu ?).
    let couleur = &vue.rgba[..4];
    let unis = vue.rgba.chunks_exact(4).filter(|p| *p == couleur).count();
    let changes = vue.rgba.chunks_exact(4).zip(otee.rgba.chunks_exact(4)).filter(|(a, b)| a != b).count();
    let n = (L * H) as f64;
    println!(
        "fenêtre d'essai sous la souris : {:.0} % de ses pixels d'une même couleur dans la capture ; ôtée de la \
         capture, {:.0} % changés ({} fenêtre ôtée)",
        unis as f64 / n * 100.0,
        changes as f64 / n * 100.0,
        otees.len()
    );
    match imp::ecran_de_la_souris() {
        Ok(r) => println!(
            "« regarde mon écran » prendrait l'écran de la souris : {}×{} pixels",
            r.right - r.left,
            r.bottom - r.top
        ),
        Err(e) => println!("l'écran de la souris : {e}"),
    }
    let vue_entiere = unis as f64 / n > 0.9;
    let disparue = changes as f64 / n > 0.5;
    println!(
        "{}",
        match (vue_entiere, disparue) {
            (true, true) => "ôtée : la capture voit ce que la fenêtre cachait",
            (true, false) => "PAS ôtée : la fenêtre reste dans la capture (ou ce qu'elle cachait est de sa couleur)",
            (false, _) => "indécis : la fenêtre d'essai ne s'est pas vue en entier (une fenêtre toujours devant ?)",
        }
    );
    i32::from(!(vue_entiere && disparue))
}
