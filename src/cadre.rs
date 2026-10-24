//! La barre de titre que dessine Eve : la fenêtre native n'a plus la barre de Windows au-dessus de la sienne.
//!
//! Windows garde le cadre de la fenêtre — ses bords, qui la redimensionnent, son ombre, son liseré, ses coins arrondis
//! — mais sa zone client monte jusqu'en haut (`WM_NCCALCSIZE`) : la barre d'Eve y prend la place de la sienne, avec ses
//! boutons à elle, réduire, agrandir ou restaurer, fermer ([`crate::natif`] les dessine). Pour Windows, cette barre
//! reste une barre de titre (`WM_NCHITTEST`) : la glisser déplace la fenêtre (et l'agrandit contre le haut de l'écran),
//! la double-cliquer l'agrandit, son clic droit ouvre le menu de la fenêtre (comme Alt+Espace), son bord du haut la
//! redimensionne ; ses boutons sont ceux de Windows (`HTMINBUTTON`, `HTMAXBUTTON`, `HTCLOSE`), d'où les dispositions de
//! Windows 11 au survol du bouton agrandir. Ce qui se clique dans la barre — les boutons et les pastilles d'Eve, un
//! panneau ouvert par-dessus — reste à egui.
//!
//! La fenêtre est celle de winit : elle est sous-classée (`SetWindowSubclass`, sur son fil), et ce que la barre ne
//! prend pas passe à winit tel quel. La fenêtre décrit sa barre à chaque image ([`decrire`]) ; ce module lui rend l'état
//! de ses boutons, survolé ou enfoncé ([`survole`], [`enfonce`]), que Windows ne dit qu'au sous-classement.
//! `eve --essai-cadre` met le tout à l'épreuve.

use std::sync::{Mutex, PoisonError};

use eframe::egui::{Pos2, Rect};

/// Les boutons de la fenêtre, dans la barre d'Eve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bouton {
    Reduire,
    Agrandir,
    Fermer,
}

/// La barre, telle que la fenêtre la décrit à chaque image ([`decrire`]) : en points d'egui, depuis le coin haut gauche
/// de la zone client. Hors de ce qui s'y clique et de ses boutons, elle déplace la fenêtre.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Barre {
    /// Sa hauteur : la barre va de 0 à `hauteur`.
    pub hauteur: f32,
    /// Ce qui s'y clique dans egui : les boutons et les pastilles d'Eve, ce qui s'ouvre par-dessus.
    pub client: Vec<Rect>,
    /// Les boutons de la fenêtre (aucun en plein écran).
    pub boutons: Vec<(Bouton, Rect)>,
    /// Les pixels d'un point : l'échelle de l'écran, le zoom d'egui compris.
    pub echelle: f32,
}

/// Où tombe un point de la zone client, pour Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Sous la barre, ou ce qui s'y clique : à egui.
    Client,
    /// La barre : la glisser déplace la fenêtre.
    Barre,
    /// Le bord du haut et ses coins : ils redimensionnent la fenêtre.
    Haut,
    HautGauche,
    HautDroite,
    /// Un bouton de la fenêtre.
    Bouton(Bouton),
}

/// La zone du point `p` de la zone client, en points : `largeur`, celle de la zone client ; `bord`, l'épaisseur du bord
/// qui redimensionne ; agrandie, la fenêtre n'en a plus (rien ne se redimensionne, et le bouton fermer touche le coin
/// de l'écran). Ce qui se clique dans egui passe d'abord — un panneau ouvert par-dessus la barre est à lui ; puis le
/// bord du haut, sur toute la largeur, boutons compris, comme chez Windows, et ses coins sur deux épaisseurs ; puis les
/// boutons.
pub fn zone(barre: &Barre, p: Pos2, largeur: f32, bord: f32, agrandie: bool) -> Zone {
    if !(0.0..barre.hauteur).contains(&p.y) || barre.client.iter().any(|r| r.contains(p)) {
        return Zone::Client;
    }
    if !agrandie && p.y < bord {
        let coin = 2.0 * bord;
        return if p.x < coin {
            Zone::HautGauche
        } else if p.x >= largeur - coin {
            Zone::HautDroite
        } else {
            Zone::Haut
        };
    }
    match barre.boutons.iter().find(|(_, r)| r.contains(p)) {
        Some((b, _)) => Zone::Bouton(*b),
        None => Zone::Barre,
    }
}

/// Ce que la souris fait aux boutons de la fenêtre : celui qu'elle survole, celui qu'on a enfoncé. Un bouton agit quand
/// on le relâche là où on l'a enfoncé, comme ceux de Windows ; quitter la barre les relâche tous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Souris {
    survol: Option<Bouton>,
    enfonce: Option<Bouton>,
}

impl Souris {
    /// La souris passe sur `ici` (un bouton, ou le reste de la barre).
    fn bouger(&mut self, ici: Option<Bouton>) {
        self.survol = ici;
    }

    /// Elle quitte la barre, vers egui ou hors de la fenêtre.
    fn partir(&mut self) {
        *self = Souris::default();
    }

    /// Un bouton de la souris s'enfonce sur `ici`.
    fn enfoncer(&mut self, ici: Option<Bouton>) {
        self.survol = ici;
        self.enfonce = ici;
    }

    /// Il se relâche sur `ici` : le bouton qui agit, s'il est celui qu'on avait enfoncé.
    fn relacher(&mut self, ici: Option<Bouton>) -> Option<Bouton> {
        self.enfonce.take().filter(|b| Some(*b) == ici)
    }
}

/// Ce que partagent la fenêtre et son sous-classement, sur le fil de la fenêtre : la barre décrite, la souris, et si
/// Windows suit la souris hors de la zone client (il dira quand elle en sort : `WM_NCMOUSELEAVE`).
struct Partage {
    barre: Option<Barre>,
    souris: Souris,
    suivie: bool,
}

static PARTAGE: Mutex<Partage> =
    Mutex::new(Partage { barre: None, souris: Souris { survol: None, enfonce: None }, suivie: false });

/// Le partage, même après une panique d'egui sous le verrou : un rappel de Windows ne doit jamais paniquer.
fn partage() -> std::sync::MutexGuard<'static, Partage> {
    PARTAGE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// La barre telle que la fenêtre la dessine, à chaque image.
pub fn decrire(barre: Barre) {
    partage().barre = Some(barre);
}

/// La dernière barre décrite (l'essai du cadre y prend ses points).
pub fn barre() -> Option<Barre> {
    partage().barre.clone()
}

/// Le bouton de la fenêtre que survole la souris.
pub fn survole() -> Option<Bouton> {
    partage().souris.survol
}

/// Le bouton de la fenêtre enfoncé (il ne paraît enfoncé que survolé, comme ceux de Windows).
pub fn enfonce() -> Option<Bouton> {
    partage().souris.enfonce
}

#[cfg(windows)]
pub use imp::{installer, poignee};

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use eframe::egui::pos2;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, ScreenToClient,
    };
    use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL,
    };
    use windows::Win32::UI::Shell::{
        ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA,
        DefSubclassProc, RemoveWindowSubclass, SHAppBarMessage, SetWindowSubclass,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, EnableMenuItem, GWL_EXSTYLE, GWL_STYLE, GetClientRect, GetSystemMenu, GetWindowLongW,
        GetWindowRect, HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsIconic,
        IsZoomed, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, NCCALCSIZE_PARAMS, PostMessageW, SC_CLOSE, SC_MAXIMIZE,
        SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SM_CXPADDEDBORDER, SM_CYSIZEFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_DPICHANGED, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
        WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCRBUTTONUP, WM_SYSCHAR, WM_SYSCOMMAND,
        WS_CAPTION,
    };

    use super::{Bouton, Zone, partage, zone};

    /// Le numéro du sous-classement d'Eve (« Eve », un seul par fenêtre).
    const ID: usize = 0x0045_7665;
    /// Ce qu'une barre des tâches qui se cache garde d'une fenêtre agrandie, de son côté, en pixels (comme le Terminal
    /// de Windows) : sans ce peu, Windows croirait la fenêtre en plein écran, et la barre ne se montrerait plus.
    const BARRE_DES_TACHES: i32 = 2;

    /// La fenêtre sous-classée (0 : aucune).
    static FENETRE: AtomicIsize = AtomicIsize::new(0);
    /// Ce qui fait redessiner la fenêtre quand un bouton change d'état.
    static REVEIL: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

    /// La fenêtre sous-classée (l'essai du cadre l'interroge comme Windows).
    pub fn poignee() -> Option<HWND> {
        match FENETRE.load(Ordering::SeqCst) {
            0 => None,
            f => Some(HWND(f as *mut core::ffi::c_void)),
        }
    }

    /// La barre de titre devient celle d'Eve : sous-classe la fenêtre `hwnd` (ouverte, sur ce fil), puis fait refaire
    /// son cadre à Windows, sans la bouger ni la redimensionner. `reveil` la fait redessiner (un bouton change d'état).
    /// `false` si Windows refuse : sa barre reste.
    pub fn installer(hwnd: HWND, reveil: Box<dyn Fn() + Send + Sync>) -> bool {
        let _ = REVEIL.set(reveil);
        // SAFETY : la fenêtre est ouverte et appartient à ce fil ; le sous-classement se retire à sa destruction
        // (WM_NCDESTROY, dans `procedure`).
        if !unsafe { SetWindowSubclass(hwnd, Some(procedure), ID, 0) }.as_bool() {
            return false;
        }
        FENETRE.store(hwnd.0 as isize, Ordering::SeqCst);
        let rien_d_autre = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        // SAFETY : le cadre de la fenêtre se recalcule (WM_NCCALCSIZE) ; ni place, ni taille, ni rang ne changent.
        let _ = unsafe { SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | rien_d_autre) };
        true
    }

    /// La fenêtre d'Eve, avec sa barre à elle : pas une autre, ni elle en plein écran (winit lui ôte alors son cadre,
    /// et tout redevient client).
    fn encadree(hwnd: HWND) -> bool {
        // SAFETY : une lecture du style de la fenêtre de ce fil.
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        FENETRE.load(Ordering::SeqCst) == hwnd.0 as isize && style & WS_CAPTION.0 == WS_CAPTION.0
    }

    fn agrandie(hwnd: HWND) -> bool {
        // SAFETY : une lecture de l'état de la fenêtre.
        unsafe { IsZoomed(hwnd) }.as_bool()
    }

    /// L'épaisseur du bord qui redimensionne, en pixels, à l'échelle de l'écran de la fenêtre : celle que Windows cache
    /// hors de l'écran quand la fenêtre est agrandie.
    fn bord(hwnd: HWND) -> i32 {
        // SAFETY : des lectures ; une échelle inconnue (0) vaut 100 %.
        unsafe {
            let dpi = match GetDpiForWindow(hwnd) {
                0 => 96,
                d => d,
            };
            GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        }
    }

    fn reveiller() {
        if let Some(r) = REVEIL.get() {
            r();
        }
    }

    /// Le bouton de la fenêtre que nomme un code de Windows (`HTMINBUTTON`…), passé dans `wparam` des messages de la
    /// souris hors de la zone client.
    fn bouton_de(wparam: WPARAM) -> Option<Bouton> {
        match wparam.0 as u32 {
            HTMINBUTTON => Some(Bouton::Reduire),
            HTMAXBUTTON => Some(Bouton::Agrandir),
            HTCLOSE => Some(Bouton::Fermer),
            _ => None,
        }
    }

    /// Le code de Windows d'une zone de la barre.
    pub(super) fn code(z: Zone) -> u32 {
        match z {
            Zone::Client => HTCLIENT,
            Zone::Barre => HTCAPTION,
            Zone::Haut => HTTOP,
            Zone::HautGauche => HTTOPLEFT,
            Zone::HautDroite => HTTOPRIGHT,
            Zone::Bouton(Bouton::Reduire) => HTMINBUTTON,
            Zone::Bouton(Bouton::Agrandir) => HTMAXBUTTON,
            Zone::Bouton(Bouton::Fermer) => HTCLOSE,
        }
    }

    /// Les messages de la fenêtre, avant winit. Un rappel de Windows qui paniquerait arrêterait tout le programme :
    /// rien ici ne panique (ni indice, ni `unwrap`).
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _donnee: usize,
    ) -> LRESULT {
        // SAFETY : Windows appelle ce rappel sur le fil de la fenêtre, avec les paramètres du message ; le suivant
        // (winit) les reçoit tels quels.
        let suivant = || unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        match msg {
            WM_NCCALCSIZE if encadree(hwnd) => {
                // SAFETY : les paramètres de WM_NCCALCSIZE, tels que Windows les donne.
                unsafe { calculer(hwnd, wparam, lparam) }
            }
            WM_NCHITTEST => toucher(hwnd, suivant(), lparam),
            WM_NCMOUSEMOVE => {
                bouger(hwnd, bouton_de(wparam));
                suivant()
            }
            WM_NCMOUSELEAVE => {
                partir();
                suivant()
            }
            // Les boutons de la fenêtre se suivent ici, pas chez Windows : sa boucle d'un bouton enfoncé dessinerait
            // ses anciens boutons par-dessus ceux d'Eve. Le double clic est un second clic.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if bouton_de(wparam).is_some() => {
                partage().souris.enfoncer(bouton_de(wparam));
                reveiller();
                LRESULT(0)
            }
            WM_NCLBUTTONUP if bouton_de(wparam).is_some() => {
                relacher(hwnd, bouton_de(wparam), lparam);
                LRESULT(0)
            }
            // Le clic droit sur la barre : le menu de la fenêtre. Windows ne l'ouvre pas de lui-même sur une barre qui
            // est dans la zone client (essayé : ni au clic droit, ni à WM_CONTEXTMENU) ; le Terminal de Windows l'ouvre
            // aussi lui-même.
            WM_NCRBUTTONUP if wparam.0 as u32 == HTCAPTION && encadree(hwnd) => {
                ouvrir_le_menu(hwnd, lparam);
                LRESULT(0)
            }
            // Alt+Espace : winit garde le caractère pour ses touches, et Windows n'ouvre le menu de la fenêtre qu'à sa
            // vue ; il le reçoit ici, egui non (l'espace ne fait pas parler Eve).
            WM_SYSCHAR if menu_au_clavier(wparam, lparam) => {
                // SAFETY : le message tel que Windows l'a donné, à son traitement d'office.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            WM_DPICHANGED => {
                let r = suivant();
                apres_l_echelle(hwnd);
                r
            }
            WM_NCDESTROY => {
                // SAFETY : le sous-classement posé par `installer`, retiré par le fil de la fenêtre avant qu'elle
                // disparaisse.
                let _ = unsafe { RemoveWindowSubclass(hwnd, Some(procedure), ID) };
                FENETRE.store(0, Ordering::SeqCst);
                suivant()
            }
            _ => suivant(),
        }
    }

    /// WM_NCCALCSIZE : Windows ôte de la fenêtre ses bords et sa barre de titre ; la zone client reprend la hauteur de
    /// la barre, et le bord du haut, qui redimensionne encore (voir `toucher`). Agrandie, la fenêtre déborde de l'écran
    /// de l'épaisseur de ses bords, cachés : la zone client prend la zone de travail de son écran, sans la barre des
    /// tâches.
    ///
    /// # Safety
    /// `wparam` et `lparam` sont ceux de WM_NCCALCSIZE : `lparam` désigne un NCCALCSIZE_PARAMS (`wparam` vrai) ou un
    /// RECT, à remplir de la zone client.
    unsafe fn calculer(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let rect: *mut RECT = if wparam.0 != 0 {
            // SAFETY : voir la fonction ; le premier rectangle est celui de la fenêtre, à changer en zone client.
            unsafe { (&raw mut (*(lparam.0 as *mut NCCALCSIZE_PARAMS)).rgrc).cast() }
        } else {
            lparam.0 as *mut RECT
        };
        // SAFETY : `rect` vient de Windows, valide le temps du message.
        let propose = unsafe { *rect };
        // SAFETY : le message tel que Windows l'a donné.
        let r = unsafe { DefSubclassProc(hwnd, WM_NCCALCSIZE, wparam, lparam) };
        if r.0 != 0 {
            return r;
        }
        // SAFETY : comme plus haut.
        let mut client = unsafe { *rect };
        if agrandie(hwnd) {
            client = travail(&propose).unwrap_or(RECT { top: propose.top + bord(hwnd), ..client });
        } else {
            client.top = propose.top;
        }
        // SAFETY : comme plus haut.
        unsafe { *rect = client };
        LRESULT(0)
    }

    /// La zone de travail de l'écran de la fenêtre (`fenetre`, son rectangle : c'est lui qui dit l'écran où elle
    /// s'agrandit), moins ce que garde une barre des tâches qui se cache.
    fn travail(fenetre: &RECT) -> Option<RECT> {
        // SAFETY : des lectures ; `info` et `fenetre` vivent pendant les appels.
        let info = unsafe {
            let ecran = MonitorFromRect(fenetre, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(ecran, &raw mut info).as_bool().then_some(info)?
        };
        let mut t = info.rcWork;
        let mut etat = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
        // SAFETY : `etat` vit pendant l'appel, sa taille est celle qu'on annonce.
        if unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut etat) } as u32 & ABS_AUTOHIDE == 0 {
            return Some(t);
        }
        let masquee = |cote: u32| {
            let mut d = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                uEdge: cote,
                rc: info.rcMonitor,
                ..Default::default()
            };
            // SAFETY : comme plus haut ; Windows rend la barre des tâches de ce côté de cet écran, s'il y en a une.
            unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &raw mut d) != 0 }
        };
        if masquee(ABE_TOP) {
            t.top += BARRE_DES_TACHES;
        }
        if masquee(ABE_BOTTOM) {
            t.bottom -= BARRE_DES_TACHES;
        }
        if masquee(ABE_LEFT) {
            t.left += BARRE_DES_TACHES;
        }
        if masquee(ABE_RIGHT) {
            t.right -= BARRE_DES_TACHES;
        }
        Some(t)
    }

    /// WM_NCHITTEST : Windows garde ce qu'il trouve hors de la zone client (les bords de gauche, de droite et du bas) ;
    /// dans la zone client, la barre dit où tombe le point ([`zone`]).
    fn toucher(hwnd: HWND, defaut: LRESULT, lparam: LPARAM) -> LRESULT {
        if defaut.0 != HTCLIENT as isize || !encadree(hwnd) {
            return defaut;
        }
        // Le point, en pixels de l'écran (signés : un écran à gauche du principal a des abscisses négatives).
        let mut p = POINT { x: i32::from(lparam.0 as u16 as i16), y: i32::from((lparam.0 >> 16) as u16 as i16) };
        let mut client = RECT::default();
        // SAFETY : des lectures sur la fenêtre de ce fil ; `p` et `client` vivent pendant les appels.
        let lu = unsafe { ScreenToClient(hwnd, &raw mut p).as_bool() && GetClientRect(hwnd, &raw mut client).is_ok() };
        if !lu {
            return defaut;
        }
        let (bord, agrandie) = (bord(hwnd), agrandie(hwnd));
        let etat = partage();
        let Some(barre) = &etat.barre else { return defaut };
        let e = barre.echelle.max(0.25);
        let point = pos2((p.x as f32 + 0.5) / e, (p.y as f32 + 0.5) / e);
        LRESULT(code(zone(barre, point, client.right as f32 / e, bord as f32 / e, agrandie)) as isize)
    }

    /// WM_NCMOUSEMOVE : la souris passe sur un bouton de la fenêtre, ou ailleurs hors de la zone client. Sur un bouton,
    /// Windows la suit, pour dire quand elle s'en va.
    fn bouger(hwnd: HWND, ici: Option<Bouton>) {
        let (change, suivre) = {
            let mut p = partage();
            let avant = p.souris;
            p.souris.bouger(ici);
            let suivre = ici.is_some() && !p.suivie;
            p.suivie |= suivre;
            (p.souris != avant, suivre)
        };
        if suivre {
            let mut t = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            // SAFETY : `t` vit pendant l'appel, sa taille est celle qu'on annonce.
            if unsafe { TrackMouseEvent(&raw mut t) }.is_err() {
                partage().suivie = false;
            }
        }
        if change {
            reveiller();
        }
    }

    /// WM_NCMOUSELEAVE : la souris a quitté la barre ; plus rien n'est survolé ni enfoncé.
    fn partir() {
        let change = {
            let mut p = partage();
            let avant = p.souris;
            p.souris.partir();
            p.suivie = false;
            p.souris != avant
        };
        if change {
            reveiller();
        }
    }

    /// WM_NCLBUTTONUP sur un bouton de la fenêtre : relâché là où il a été enfoncé, il agit, comme celui de Windows —
    /// la commande de la fenêtre, avec la place du clic, postée (elle part une fois ce message rendu).
    fn relacher(hwnd: HWND, ici: Option<Bouton>, lparam: LPARAM) {
        let agit = {
            let mut p = partage();
            let agit = p.souris.relacher(ici);
            // La fenêtre va changer de place ou s'en aller : le survol se refait au prochain mouvement.
            if agit.is_some() {
                p.souris.survol = None;
            }
            agit
        };
        reveiller();
        let commande = match agit {
            None => return,
            Some(Bouton::Reduire) => SC_MINIMIZE,
            Some(Bouton::Agrandir) if agrandie(hwnd) => SC_RESTORE,
            Some(Bouton::Agrandir) => SC_MAXIMIZE,
            Some(Bouton::Fermer) => SC_CLOSE,
        };
        // SAFETY : un message posté à la fenêtre de ce fil.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(commande as usize), lparam) };
    }

    /// Le menu de la fenêtre au point `lparam` de l'écran, ses entrées réglées comme Windows le fait — restaurer,
    /// agrandie ; déplacer, la taille et agrandir, rendue — ; le choix part comme une commande de la fenêtre, posté.
    fn ouvrir_le_menu(hwnd: HWND, lparam: LPARAM) {
        let (x, y) = (i32::from(lparam.0 as u16 as i16), i32::from((lparam.0 >> 16) as u16 as i16));
        let agrandie = agrandie(hwnd);
        // SAFETY : le menu de la fenêtre de ce fil, montré par ce fil ; le choix revient à la fermeture du menu.
        unsafe {
            let menu = GetSystemMenu(hwnd, false);
            if menu.is_invalid() {
                return;
            }
            let entrees = [
                (SC_RESTORE, agrandie),
                (SC_MOVE, !agrandie),
                (SC_SIZE, !agrandie),
                (SC_MINIMIZE, true),
                (SC_MAXIMIZE, !agrandie),
                (SC_CLOSE, true),
            ];
            for (commande, permise) in entrees {
                let _ = EnableMenuItem(menu, commande, MF_BYCOMMAND | if permise { MF_ENABLED } else { MF_GRAYED });
            }
            let choix = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, None, hwnd, None);
            if choix.0 != 0 {
                let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(choix.0 as usize), LPARAM(0));
            }
        }
    }

    /// Alt+Espace, sans Ctrl (Alt Gr est Ctrl+Alt : son espace s'écrit).
    fn menu_au_clavier(wparam: WPARAM, lparam: LPARAM) -> bool {
        let alt = (lparam.0 >> 29) & 1 == 1;
        // SAFETY : une lecture de l'état du clavier de ce fil.
        let ctrl = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
        wparam.0 == 0x20 && alt && !ctrl
    }

    /// Après un changement d'échelle (la fenêtre passe sur un écran d'une autre échelle) : winit refait la taille de la
    /// fenêtre d'après sa zone client, en lui ajoutant le haut d'un cadre de Windows — sa barre de titre et son bord —
    /// qu'elle a déjà dans sa zone client ; la fenêtre grandirait d'autant à chaque passage. On le lui retire aussitôt.
    fn apres_l_echelle(hwnd: HWND) {
        // SAFETY : une lecture de l'état de la fenêtre.
        if !encadree(hwnd) || agrandie(hwnd) || unsafe { IsIconic(hwnd) }.as_bool() {
            return;
        }
        let mut haut = RECT::default();
        let mut f = RECT::default();
        // SAFETY : des lectures sur la fenêtre de ce fil, puis sa taille, sans la bouger ni l'activer.
        unsafe {
            let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
            let etendu = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
            // Le calcul même de winit : le cadre de Windows autour d'un rectangle vide.
            if AdjustWindowRectExForDpi(&raw mut haut, style, false, etendu, GetDpiForWindow(hwnd)).is_err()
                || GetWindowRect(hwnd, &raw mut f).is_err()
                || haut.top >= 0
            {
                return;
            }
            let hauteur = (f.bottom - f.top + haut.top).max(1);
            let _ =
                SetWindowPos(hwnd, None, 0, 0, f.right - f.left, hauteur, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

#[cfg(test)]
mod essais {
    use eframe::egui::{pos2, vec2};

    use super::*;

    /// Une barre de 90 points de haut sur 1000 de large, à l'échelle 1,5 : les trois boutons de la fenêtre à droite
    /// (46 sur 32), deux boutons d'Eve dans sa rangée, et un panneau ouvert qui déborde sur le bas de la barre.
    fn barre() -> Barre {
        let bouton = |droite: f32| Rect::from_min_size(pos2(droite - 46.0, 0.0), vec2(46.0, 32.0));
        Barre {
            hauteur: 90.0,
            client: vec![
                Rect::from_min_size(pos2(880.0, 47.0), vec2(100.0, 27.0)),
                Rect::from_min_size(pos2(740.0, 47.0), vec2(130.0, 27.0)),
                Rect::from_min_size(pos2(606.0, 82.0), vec2(380.0, 400.0)),
            ],
            boutons: vec![
                (Bouton::Fermer, bouton(1000.0)),
                (Bouton::Agrandir, bouton(954.0)),
                (Bouton::Reduire, bouton(908.0)),
            ],
            echelle: 1.5,
        }
    }

    #[test]
    fn chaque_point_de_la_barre_a_sa_zone() {
        let b = barre();
        let z = |x: f32, y: f32| zone(&b, pos2(x, y), 1000.0, 8.0, false);
        // Les boutons de la fenêtre, sous le bord du haut.
        assert_eq!(z(980.0, 20.0), Zone::Bouton(Bouton::Fermer));
        assert_eq!(z(930.0, 20.0), Zone::Bouton(Bouton::Agrandir));
        assert_eq!(z(885.0, 31.0), Zone::Bouton(Bouton::Reduire));
        // Ce qui se clique dans egui, et ce qui s'ouvre par-dessus le bas de la barre.
        assert_eq!(z(900.0, 60.0), Zone::Client);
        assert_eq!(z(800.0, 60.0), Zone::Client);
        assert_eq!(z(700.0, 86.0), Zone::Client);
        // Le reste de la barre la déplace : la bande, l'adresse, le logo, entre les boutons d'Eve.
        assert_eq!(z(500.0, 20.0), Zone::Barre);
        assert_eq!(z(30.0, 16.0), Zone::Barre);
        assert_eq!(z(40.0, 60.0), Zone::Barre);
        assert_eq!(z(875.0, 60.0), Zone::Barre);
        // Sous la barre : egui.
        assert_eq!(z(500.0, 90.0), Zone::Client);
        assert_eq!(z(500.0, 400.0), Zone::Client);
    }

    #[test]
    fn le_bord_du_haut_redimensionne_sauf_agrandie() {
        let b = barre();
        let z = |x: f32, y: f32, agrandie: bool| zone(&b, pos2(x, y), 1000.0, 8.0, agrandie);
        assert_eq!(z(500.0, 0.0, false), Zone::Haut);
        assert_eq!(z(500.0, 7.9, false), Zone::Haut);
        assert_eq!(z(500.0, 8.0, false), Zone::Barre);
        // Ses coins, sur deux épaisseurs ; le bord passe avant les boutons de la fenêtre, comme chez Windows.
        assert_eq!(z(15.0, 3.0, false), Zone::HautGauche);
        assert_eq!(z(16.0, 3.0, false), Zone::Haut);
        assert_eq!(z(990.0, 3.0, false), Zone::HautDroite);
        assert_eq!(z(960.0, 3.0, false), Zone::Haut);
        // Agrandie : plus de bord, et le bouton fermer touche le coin de l'écran.
        assert_eq!(z(999.0, 0.0, true), Zone::Bouton(Bouton::Fermer));
        assert_eq!(z(500.0, 0.0, true), Zone::Barre);
        assert_eq!(z(0.0, 0.0, true), Zone::Barre);
    }

    #[test]
    fn sans_boutons_la_barre_ne_fait_que_deplacer() {
        // En plein écran, la fenêtre ne dessine pas ses boutons (et Windows ne demande plus rien à la barre).
        let b = Barre { boutons: Vec::new(), ..barre() };
        assert_eq!(zone(&b, pos2(980.0, 20.0), 1000.0, 8.0, false), Zone::Barre);
        // Une barre pas encore décrite (aucune hauteur) : tout est à egui.
        assert_eq!(zone(&Barre::default(), pos2(10.0, 10.0), 1000.0, 8.0, false), Zone::Client);
    }

    #[test]
    fn un_bouton_agit_relache_la_ou_il_a_ete_enfonce() {
        let mut s = Souris::default();
        s.bouger(Some(Bouton::Fermer));
        assert_eq!(s.survol, Some(Bouton::Fermer));
        s.enfoncer(Some(Bouton::Fermer));
        assert_eq!(s.relacher(Some(Bouton::Fermer)), Some(Bouton::Fermer));
        assert_eq!(s.enfonce, None);
        // Enfoncé sur l'un, relâché sur l'autre : rien.
        s.enfoncer(Some(Bouton::Agrandir));
        assert_eq!(s.relacher(Some(Bouton::Reduire)), None);
        // Enfoncé, passé sur la barre, revenu : il agit encore.
        s.enfoncer(Some(Bouton::Reduire));
        s.bouger(None);
        s.bouger(Some(Bouton::Reduire));
        assert_eq!(s.relacher(Some(Bouton::Reduire)), Some(Bouton::Reduire));
        // Enfoncé, puis la souris quitte la barre : relâché ailleurs, rien.
        s.enfoncer(Some(Bouton::Fermer));
        s.partir();
        assert_eq!(s, Souris::default());
        assert_eq!(s.relacher(Some(Bouton::Fermer)), None);
        // Relâché sur un bouton jamais enfoncé (le clic venait d'egui) : rien.
        assert_eq!(s.relacher(Some(Bouton::Agrandir)), None);
    }

    #[cfg(windows)]
    #[test]
    fn chaque_zone_a_le_code_de_windows() {
        use windows::Win32::UI::WindowsAndMessaging::{
            HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT,
        };
        let codes = [
            (Zone::Client, HTCLIENT),
            (Zone::Barre, HTCAPTION),
            (Zone::Haut, HTTOP),
            (Zone::HautGauche, HTTOPLEFT),
            (Zone::HautDroite, HTTOPRIGHT),
            (Zone::Bouton(Bouton::Reduire), HTMINBUTTON),
            (Zone::Bouton(Bouton::Agrandir), HTMAXBUTTON),
            (Zone::Bouton(Bouton::Fermer), HTCLOSE),
        ];
        for (z, attendu) in codes {
            assert_eq!(imp::code(z), attendu, "{z:?}");
        }
    }
}
