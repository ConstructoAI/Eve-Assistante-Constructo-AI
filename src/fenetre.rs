//! La fenêtre d'Eve (Edge en mode application) vue par Windows : la retrouver, l'afficher ou la cacher par un raccourci
//! global (Ctrl+Alt+E), la garder devant les autres, et le démarrage d'Eve avec Windows.
//!
//! La fenêtre n'est pas à Eve : c'est une fenêtre d'Edge (`ecran::ouvrir_fenetre`), retrouvée par sa classe et par
//! son titre, le `<title>` de la page. Windows ne laisse un programme mettre au premier plan la fenêtre d'un autre que
//! s'il vient de recevoir une entrée de la personne ; le message de son raccourci global (WM_HOTKEY) en est une. D'où
//! `basculer`, appelé par le fil du raccourci dès le message reçu, avec deux replis si Windows refuse quand même.
//!
//! Le démarrage avec Windows passe par la clé « Run » de l'utilisateur (HKCU : ni droit d'administrateur, ni tâche
//! planifiée), qui lance `Eve.bat` à l'ouverture de la session.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::journal;

/// Le titre de la fenêtre (le `<title>` de la page, que pose `ecran`) pour ce nom affiché : c'est par lui qu'on la
/// retrouve. Deux Eve de noms différents sur un même poste ne prennent pas la fenêtre l'une de l'autre.
pub fn titre_pour(nom: &str) -> String {
    format!("Eve — {nom}")
}
/// La classe des fenêtres de premier niveau d'Edge (celle de Chromium).
const CLASSE: &str = "Chrome_WidgetWin_1";
/// Le programme qu'ouvre `ecran::ouvrir_fenetre` : sa fenêtre passe avant celle d'un autre navigateur.
const EDGE: &str = "msedge.exe";
/// La clé « Run » de l'utilisateur, et la valeur d'Eve.
const CLE_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Le nom de la valeur sous la clé « Run » : une par Eve, d'après son nom affiché — deux Eve d'un même poste ne
/// s'effacent pas l'une l'autre.
fn valeur_run() -> String {
    valeur_run_pour(crate::reglages::nom_affiche())
}

fn valeur_run_pour(nom: &str) -> String {
    if nom == "Adjointe" { "Eve (Entrepreneur Général)".into() } else { format!("Eve ({nom})") }
}
/// Le lanceur, cherché au-dessus de l'exécutable : trois dossiers depuis `target\release\`, deux depuis `bin\`, quatre
/// depuis l'exécutable d'un essai (`target\release\deps\`).
const LANCEUR: &str = "Eve.bat";
const REMONTEE: usize = 4;
/// « Toujours devant » attend la fenêtre (Edge s'ouvre en une ou deux secondes) : un essai toutes les 300 ms, 20 s au
/// plus.
const PAS_DEVANT: Duration = Duration::from_millis(300);
const DELAI_DEVANT: Duration = Duration::from_secs(20);

/// Le dernier « toujours devant » demandé.
static DEVANT: Relais = Relais::nouveau();

/// Affiche la fenêtre d'Eve et la met au premier plan ; si elle l'est déjà, la réduit. Sans fenêtre : une ligne au
/// journal. À appeler dès le message du raccourci (WM_HOTKEY) : c'est lui qui donne le droit de passer devant.
pub fn basculer() {
    imp::basculer();
}

/// Affiche la fenêtre d'Eve et la met au premier plan, sans jamais la réduire (« Eve, scanne » : la caméra s'ouvre
/// devant la personne). Windows peut refuser le premier plan : son bouton clignote alors dans la barre des tâches.
pub fn montrer() {
    imp::montrer();
}

/// Écrit une valeur texte dans le registre de la personne (`HKCU\cle`, la clé créée au besoin) : l'inscription d'Eve
/// auprès de Windows pour ses notifications (`notifications.rs`).
pub(crate) fn ecrire_au_registre(cle: &str, nom: &str, texte: &str) -> Result<(), String> {
    imp::ecrire_valeur(cle, nom, texte)
}

/// Efface du registre de la personne la clé `HKCU\cle` et ce qu'elle contient ; absente, rien.
pub(crate) fn effacer_du_registre(cle: &str) -> Result<(), String> {
    imp::effacer_cle(cle)
}

/// Toujours devant les autres fenêtres, ou non. La fenêtre peut ne pas exister encore (Edge s'ouvre) : un fil réessaie
/// quelques secondes.
pub fn premier_plan(oui: bool) {
    let moi = DEVANT.nouveau_tour();
    thread::spawn(move || {
        let issue = DEVANT.reessayer(moi, DELAI_DEVANT, PAS_DEVANT, imp::trouver, |f| imp::poser_devant(f, oui));
        if oui && issue == Issue::Introuvable {
            journal::ecrire("« Toujours devant » : la fenêtre d'Eve n'est pas venue en 20 secondes.");
        }
    });
}

/// Le raccourci global qui affiche ou cache Eve (Ctrl+Alt+E), dans un fil à lui.
pub fn demarrer_raccourci() {
    imp::demarrer_raccourci();
}

/// Démarrer Eve avec Windows (la clé « Run » de l'utilisateur, qui lance `Eve.bat`), ou non.
pub fn demarrage_windows(oui: bool) -> Result<(), String> {
    match inscrire(CLE_RUN, oui)? {
        Some(lanceur) => journal::ecrire(&format!("Eve démarrera avec Windows ({}).", lanceur.display())),
        None => journal::ecrire("Eve ne démarre plus avec Windows."),
    }
    Ok(())
}

/// Eve démarre-t-elle avec Windows ?
pub fn demarrage_windows_actif() -> bool {
    inscrit(CLE_RUN)
}

/// Inscrit le lanceur sous `cle` (la clé « Run », ou celle des essais), ou l'en retire (absent : pas une erreur) ; rend
/// le lanceur inscrit.
fn inscrire(cle: &str, oui: bool) -> Result<Option<PathBuf>, String> {
    if !oui {
        imp::supprimer_valeur(cle, &valeur_run())?;
        return Ok(None);
    }
    let lanceur = lanceur()?;
    imp::ecrire_valeur(cle, &valeur_run(), &commande(&lanceur))?;
    Ok(Some(lanceur))
}

/// La valeur existe sous `cle` et vise un fichier qui existe.
fn inscrit(cle: &str) -> bool {
    matches!(imp::lire_valeur(cle, &valeur_run()), Ok(Some(c)) if fichier_vise(&c).is_file())
}

/// Le lanceur d'Eve : celui du dossier d'Eve (`EVE_DOSSIER`, quand `Eve.bat` lance une copie locale du programme),
/// sinon au-dessus de l'exécutable.
fn lanceur() -> Result<PathBuf, String> {
    if let Some(d) = crate::reglages::dossier_d_eve() {
        return Ok(d.join(LANCEUR));
    }
    let exe = std::env::current_exe().map_err(|e| format!("le chemin du programme est introuvable ({e})"))?;
    chercher_lanceur(&exe)
        .ok_or_else(|| format!("{LANCEUR} est introuvable dans les {REMONTEE} dossiers au-dessus de {}", exe.display()))
}

/// `Eve.bat` dans le dossier de `exe`, puis dans ceux du dessus : `REMONTEE` dossiers en tout.
fn chercher_lanceur(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().skip(1).take(REMONTEE).map(|d| d.join(LANCEUR)).find(|p| p.is_file())
}

/// La commande de la clé « Run » : le chemin du lanceur entre guillemets (il peut porter des espaces).
fn commande(lanceur: &Path) -> String {
    format!("\"{}\"", lanceur.display())
}

/// Le fichier que vise une commande de la clé « Run » : le chemin entre guillemets, sinon la commande entière.
fn fichier_vise(commande: &str) -> PathBuf {
    let c = commande.trim();
    match c.strip_prefix('"').and_then(|reste| reste.split_once('"')) {
        Some((chemin, _)) => PathBuf::from(chemin),
        None => PathBuf::from(c),
    }
}

/// Le titre est-il celui de la fenêtre d'Eve ? (La console d'`Eve.bat`, « Eve - journal », a un tiret simple.)
fn titre_d_eve(titre: &str) -> bool {
    titre.starts_with(&titre_pour(crate::reglages::nom_affiche()))
}

/// Ce que fait le raccourci.
#[derive(Debug, PartialEq, Eq)]
enum Geste {
    Reduire,
    Restaurer,
    Afficher,
}

/// Au premier plan et pas réduite : la réduire. Sinon, la restaurer si elle est réduite (seulement alors : restaurer une
/// fenêtre agrandie la rapetisserait), l'afficher, et la mettre devant.
fn geste(devant: bool, reduite: bool) -> Geste {
    match (devant, reduite) {
        (true, false) => Geste::Reduire,
        (_, true) => Geste::Restaurer,
        (false, false) => Geste::Afficher,
    }
}

/// Le caractère que donne une touche, d'après ToUnicodeEx : `n` caractères écrits dans `tampon` (négatif : une touche
/// morte, qui en écrit un), sans les caractères de contrôle (Ctrl+E seul donne le code 5, qui n'écrit rien).
fn caractere_produit(n: i32, tampon: &[u16]) -> Option<String> {
    let n = (n.unsigned_abs() as usize).min(tampon.len());
    let texte: String = String::from_utf16_lossy(&tampon[..n]).chars().filter(|c| !c.is_control()).collect();
    (!texte.is_empty()).then_some(texte)
}

/// Le nom d'une disposition de clavier, pour le journal : sa langue (le mot du bas du numéro) quand on la connaît, puis
/// son numéro — « français (Canada) 10090C0C ».
fn nom_du_clavier(numero: u32) -> String {
    let langue = match numero & 0xFFFF {
        0x0C0C => "français (Canada) ",
        0x040C => "français (France) ",
        0x1009 => "anglais (Canada) ",
        0x0409 => "anglais (États-Unis) ",
        _ => "",
    };
    format!("{langue}{numero:08X}")
}

/// La ligne du journal quand Ctrl+Alt+E écrit un caractère avec l'une des dispositions du poste : (numéro, caractère).
fn avertissement(ecrits: &[(u32, String)]) -> Option<String> {
    if ecrits.is_empty() {
        return None;
    }
    let liste: Vec<String> =
        ecrits.iter().map(|(d, c)| format!("« {c} » avec le clavier {}", nom_du_clavier(*d))).collect();
    Some(format!(
        "Attention : Ctrl+Alt+E (AltGr+E) écrit {} ; tant qu'Eve tourne, ces touches affichent ou cachent Eve au lieu \
         d'écrire.",
        liste.join(", ")
    ))
}

/// Les appels à `premier_plan` se relaient : chacun prend un numéro (sa génération), et le fil qui attend la fenêtre
/// abandonne dès qu'un appel plus récent a pris le relais. Le verrou tient ensemble la dernière vérification du numéro
/// et le geste : un fil plus ancien ne peut pas défaire, un instant après, le geste d'un plus récent.
struct Relais {
    generation: AtomicU64,
    verrou: Mutex<()>,
}

/// Comment finit l'attente d'un fil de `premier_plan`.
#[derive(Debug, PartialEq, Eq)]
enum Issue {
    /// La fenêtre est là, et le geste est posé.
    Pose,
    /// Un appel plus récent a pris le relais.
    Remplace,
    /// La fenêtre n'est pas venue à temps.
    Introuvable,
}

impl Relais {
    const fn nouveau() -> Relais {
        Relais { generation: AtomicU64::new(0), verrou: Mutex::new(()) }
    }

    /// Un nouvel appel : son numéro.
    fn nouveau_tour(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn courant(&self, moi: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == moi
    }

    /// Cherche la fenêtre tous les `pas`, `delai` au plus, et pose le geste dès qu'elle est là — si l'appel `moi` est
    /// encore le plus récent.
    fn reessayer<T>(
        &self,
        moi: u64,
        delai: Duration,
        pas: Duration,
        mut chercher: impl FnMut() -> Option<T>,
        poser: impl FnOnce(T),
    ) -> Issue {
        let debut = Instant::now();
        loop {
            if !self.courant(moi) {
                return Issue::Remplace;
            }
            if let Some(fenetre) = chercher() {
                let _garde = self.verrou.lock().unwrap_or_else(PoisonError::into_inner);
                if !self.courant(moi) {
                    return Issue::Remplace;
                }
                poser(fenetre);
                return Issue::Pose;
            }
            if debut.elapsed() >= delai {
                return Issue::Introuvable;
            }
            thread::sleep(pas);
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_HOTKEY_ALREADY_REGISTERED, ERROR_MORE_DATA, HWND, LPARAM, TRUE,
        WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayoutList, HKL, HOT_KEY_MODIFIERS, INPUT, INPUT_MOUSE, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL,
        MOD_NOREPEAT, MapVirtualKeyExW, RegisterHotKey, SendInput, ToUnicodeEx, VK_CONTROL, VK_E, VK_LCONTROL,
        VK_LMENU, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GA_ROOTOWNER, GetAncestor, GetClassNameW, GetForegroundWindow, GetMessageW,
        GetWindowTextW, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindowVisible, MSG,
        SHOW_WINDOW_CMD, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow,
        SetWindowPos, ShowWindow, ShowWindowAsync, WM_HOTKEY,
    };
    use windows::core::{BOOL, PCWSTR, PWSTR};

    use super::{CLASSE, EDGE, Geste, avertissement, caractere_produit, geste, titre_d_eve};
    use crate::journal;

    /// Ctrl+Alt+E, une seule fois tant qu'on tient les touches.
    pub(super) const MODIFICATEURS: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0);
    pub(super) const TOUCHE: u32 = VK_E.0 as u32;
    const ID_RACCOURCI: i32 = 1;
    /// Le raccourci ne s'enregistre qu'une fois par programme.
    static RACCOURCI: AtomicBool = AtomicBool::new(false);
    /// Le drapeau 4 de ToUnicodeEx (Windows 10 1607 et plus) : la traduction laisse intact l'état du clavier, une
    /// touche morte en attente comprise.
    const SANS_TOUCHER_L_ETAT: u32 = 0x4;

    /// La fenêtre d'Eve : sa fenêtre native, dans ce programme même, si elle est ouverte ; sinon une fenêtre de premier
    /// niveau visible (réduite compte), de classe « Chrome_WidgetWin_1 », dont le titre commence par celui d'Eve
    /// (`titre_pour`) — celle d'Edge d'abord, puis dans l'ordre de Windows (la plus haute d'abord).
    pub(super) fn trouver() -> Option<HWND> {
        let toutes = fenetres();
        if let Some(native) =
            toutes.iter().copied().find(|&f| visible(f) && de_ce_programme(f) && titre_d_eve(&titre(f)))
        {
            return Some(native);
        }
        let siennes: Vec<HWND> =
            toutes.into_iter().filter(|&f| visible(f) && classe(f) == CLASSE && titre_d_eve(&titre(f))).collect();
        let edge = siennes.iter().copied().find(|&f| programme(f).is_some_and(|p| p.eq_ignore_ascii_case(EDGE)));
        edge.or_else(|| siennes.first().copied())
    }

    /// Les fenêtres de premier niveau, de la plus haute à la plus basse. Le tri se fait ensuite, hors du rappel de
    /// Windows : un rappel `extern "system"` qui paniquerait arrêterait tout le programme.
    fn fenetres() -> Vec<HWND> {
        let mut liste: Vec<HWND> = Vec::new();
        // SAFETY : EnumWindows appelle `recueillir` sur ce fil et ne rend la main qu'après le dernier appel : l'adresse de
        // `liste` qu'il lui passe reste valide, et personne d'autre n'y touche, pendant toute l'énumération.
        let _ = unsafe { EnumWindows(Some(recueillir), LPARAM((&raw mut liste) as isize)) };
        liste
    }

    unsafe extern "system" fn recueillir(f: HWND, liste: LPARAM) -> BOOL {
        // SAFETY : `liste` est l'adresse du `Vec` que prête `fenetres` (voir là).
        unsafe { (*(liste.0 as *mut Vec<HWND>)).push(f) };
        TRUE
    }

    /// Une fenêtre de ce programme : la fenêtre native d'Eve (celle d'Edge est à msedge.exe).
    fn de_ce_programme(f: HWND) -> bool {
        let mut pid = 0u32;
        // SAFETY : Windows écrit le numéro du processus dans `pid`, qui vit pendant l'appel.
        unsafe { GetWindowThreadProcessId(f, Some(&raw mut pid)) };
        pid == std::process::id()
    }

    fn visible(f: HWND) -> bool {
        // SAFETY : une lecture de l'état d'une fenêtre ; un identifiant périmé rend faux.
        unsafe { IsWindowVisible(f) }.as_bool()
    }

    /// Les `n` premiers caractères de `t` (`n` : ce que rend Windows, 0 en cas d'échec).
    fn texte(t: &[u16], n: i32) -> String {
        String::from_utf16_lossy(&t[..usize::try_from(n).unwrap_or(0).min(t.len())])
    }

    pub(super) fn classe(f: HWND) -> String {
        let mut t = [0u16; 64];
        // SAFETY : Windows écrit au plus `t.len()` caractères dans `t` et rend leur nombre (0 pour une fenêtre disparue).
        let n = unsafe { GetClassNameW(f, &mut t) };
        texte(&t, n)
    }

    pub(super) fn titre(f: HWND) -> String {
        let mut t = [0u16; 256];
        // SAFETY : comme pour la classe. Pour la fenêtre d'un autre programme, Windows rend le titre qu'il garde lui-même,
        // sans lui envoyer de message : une fenêtre qui ne répond plus ne bloque pas la recherche.
        let n = unsafe { GetWindowTextW(f, &mut t) };
        texte(&t, n)
    }

    /// Le nom du programme d'une fenêtre (« msedge.exe »).
    pub(super) fn programme(f: HWND) -> Option<String> {
        let mut pid = 0u32;
        // SAFETY : Windows écrit le numéro du processus dans `pid`, qui vit pendant l'appel.
        unsafe { GetWindowThreadProcessId(f, Some(&raw mut pid)) };
        if pid == 0 {
            return None;
        }
        // SAFETY : un droit de lecture limité sur un processus de la session ; le handle se referme plus bas.
        let processus = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut t = [0u16; 1024];
        let mut n = t.len() as u32;
        // SAFETY : le tampon fait `n` caractères ; Windows y écrit le chemin du programme et met sa longueur dans `n`.
        let lu =
            unsafe { QueryFullProcessImageNameW(processus, PROCESS_NAME_WIN32, PWSTR(t.as_mut_ptr()), &raw mut n) };
        // SAFETY : le handle ouvert ci-dessus, fermé une seule fois, lu ou non.
        let _ = unsafe { CloseHandle(processus) };
        lu.ok()?;
        let chemin = String::from_utf16_lossy(&t[..(n as usize).min(t.len())]);
        Some(chemin.rsplit('\\').next().unwrap_or_default().to_string())
    }

    /// La fenêtre d'Eve est-elle au premier plan — elle, ou une fenêtre qui lui appartient (le choix d'une pièce
    /// jointe) ?
    pub(super) fn est_devant(f: HWND) -> bool {
        // SAFETY : des lectures ; GetAncestor d'une fenêtre disparue rend un identifiant nul.
        unsafe {
            let devant = GetForegroundWindow();
            !devant.is_invalid() && (devant == f || GetAncestor(devant, GA_ROOTOWNER) == f)
        }
    }

    pub(super) fn basculer() {
        let Some(f) = trouver() else {
            journal::ecrire("Ctrl+Alt+E : la fenêtre d'Eve est introuvable.");
            return;
        };
        // SAFETY : une lecture de l'état d'une fenêtre ; un identifiant périmé rend faux.
        let reduite = unsafe { IsIconic(f) }.as_bool();
        match geste(est_devant(f), reduite) {
            Geste::Reduire => {
                // SAFETY : réduire une fenêtre de premier niveau (Windows active alors la suivante), sans attendre Edge :
                // une fenêtre qui ne répond plus ne bloque pas le raccourci.
                let _ = unsafe { ShowWindowAsync(f, SW_MINIMIZE) };
            }
            Geste::Restaurer => amener(f, SW_RESTORE, "Ctrl+Alt+E"),
            Geste::Afficher => amener(f, SW_SHOW, "Ctrl+Alt+E"),
        }
    }

    pub(super) fn montrer() {
        let Some(f) = trouver() else { return };
        if est_devant(f) {
            return;
        }
        // SAFETY : une lecture de l'état d'une fenêtre ; un identifiant périmé rend faux.
        let reduite = unsafe { IsIconic(f) }.as_bool();
        amener(f, if reduite { SW_RESTORE } else { SW_SHOW }, "La caméra");
    }

    /// Montre la fenêtre (`commande` : la restaurer, ou l'afficher), puis la met au premier plan. Si Windows refuse, deux
    /// replis, dans l'ordre :
    /// 1. une entrée vide — un événement de souris sans mouvement ni bouton, par SendInput : Windows laisse passer devant
    ///    le programme qui a fourni la dernière entrée (le geste de PowerToys). Pas une frappe Alt : la personne tient
    ///    encore Ctrl et Alt, et un Alt injecté fausserait leur état, ou ouvrirait le menu de la fenêtre qu'elle quitte ;
    /// 2. le fil de la fenêtre de premier plan (AttachThreadInput) : le temps de deux appels, notre fil partage son état
    ///    d'entrée, et Windows nous traite comme lui.
    fn amener(f: HWND, commande: SHOW_WINDOW_CMD, motif: &str) {
        // SAFETY : montrer une fenêtre de premier niveau, puis demander le premier plan ; un identifiant périmé échoue
        // sans dommage. ShowWindow attend Edge : la fenêtre doit être restaurée avant de passer devant.
        let devant = unsafe {
            let _ = ShowWindow(f, commande);
            SetForegroundWindow(f).as_bool()
        };
        if devant {
            return;
        }
        let repli = if par_entree_vide(f) {
            "une entrée vide (SendInput)"
        } else if par_fil_attache(f) {
            "le fil de la fenêtre active (AttachThreadInput)"
        } else {
            journal::ecrire(&format!(
                "{motif} : Windows refuse de mettre Eve au premier plan ; son bouton clignote dans la barre des tâches."
            ));
            return;
        };
        journal::ecrire(&format!("{motif} : Windows a d'abord refusé le premier plan, obtenu par {repli}."));
    }

    fn par_entree_vide(f: HWND) -> bool {
        let vide = INPUT { r#type: INPUT_MOUSE, ..Default::default() };
        // SAFETY : un tableau d'une entrée, de la taille annoncée ; une entrée de souris toute à zéro ne bouge ni ne
        // clique.
        unsafe {
            let _ = SendInput(&[vide], std::mem::size_of::<INPUT>() as i32);
            SetForegroundWindow(f).as_bool()
        }
    }

    fn par_fil_attache(f: HWND) -> bool {
        // SAFETY : notre fil et celui de la fenêtre de premier plan partagent leur état d'entrée le temps de deux appels,
        // puis se détachent ; sans fil de premier plan (0), ou si c'est le nôtre, rien à attacher.
        unsafe {
            let moi = GetCurrentThreadId();
            let devant = GetForegroundWindow();
            let lui = if devant.is_invalid() { 0 } else { GetWindowThreadProcessId(devant, None) };
            let attache = lui != 0 && lui != moi && AttachThreadInput(moi, lui, true).as_bool();
            let _ = BringWindowToTop(f);
            let ok = SetForegroundWindow(f).as_bool();
            if attache {
                let _ = AttachThreadInput(moi, lui, false);
            }
            ok
        }
    }

    /// Toujours devant (HWND_TOPMOST) ou non, sans déplacer, redimensionner ni activer la fenêtre.
    pub(super) fn poser_devant(f: HWND, oui: bool) {
        let rang = if oui { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY : le rang d'une fenêtre de premier niveau ; un identifiant périmé échoue sans dommage.
        if let Err(e) = unsafe { SetWindowPos(f, Some(rang), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) } {
            let quoi = if oui { "se mettre" } else { "cesser d'être" };
            journal::ecrire(&format!("La fenêtre d'Eve n'a pas pu {quoi} « toujours devant » ({e})."));
        }
    }

    pub(super) fn demarrer_raccourci() {
        if !RACCOURCI.swap(true, Ordering::SeqCst) {
            thread::spawn(raccourci);
        }
    }

    /// Le fil du raccourci : il l'enregistre pour lui-même (sans fenêtre : ses messages arrivent dans sa file), puis
    /// attend ses messages jusqu'à la fin du programme.
    fn raccourci() {
        // SAFETY : un raccourci rattaché au fil courant ; il vit autant que lui.
        if let Err(e) = unsafe { RegisterHotKey(None, ID_RACCOURCI, MODIFICATEURS, TOUCHE) } {
            if WIN32_ERROR::from_error(&e) == Some(ERROR_HOTKEY_ALREADY_REGISTERED) {
                journal::ecrire("Ctrl+Alt+E est déjà pris par un autre programme : le raccourci d'Eve ne sert pas.");
            } else {
                journal::ecrire(&format!("Le raccourci Ctrl+Alt+E ne s'enregistre pas ({e}) : il ne sert pas."));
            }
            return;
        }
        journal::ecrire("Ctrl+Alt+E affiche ou cache Eve.");
        let ecrits: Vec<(u32, String)> =
            caracteres_ctrl_alt_e().into_iter().filter_map(|(d, c)| Some((d, c?))).collect();
        if let Some(ligne) = avertissement(&ecrits) {
            journal::ecrire(&ligne);
        }
        let mut message = MSG::default();
        // SAFETY : GetMessageW remplit `message`, qui vit le temps de la boucle ; 0 (WM_QUIT) ou -1 (erreur) l'arrêtent.
        while unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0 > 0 {
            if message.message == WM_HOTKEY && message.wParam.0 == ID_RACCOURCI as usize {
                super::basculer();
            }
        }
    }

    /// Ce qu'écrit Ctrl+Alt+E (AltGr+E sur un clavier français) avec chaque disposition de clavier de la session :
    /// (le numéro de la disposition, le caractère s'il y en a un). Le raccourci prend ces touches à tous les
    /// programmes, ce caractère compris.
    pub(super) fn caracteres_ctrl_alt_e() -> Vec<(u32, Option<String>)> {
        // SAFETY : sans tableau, Windows rend seulement le nombre de dispositions de la session.
        let combien = unsafe { GetKeyboardLayoutList(None) };
        let mut dispositions = vec![HKL::default(); usize::try_from(combien).unwrap_or(0)];
        // SAFETY : Windows écrit au plus `dispositions.len()` dispositions dans le tableau, et rend combien il en a écrit.
        let n = unsafe { GetKeyboardLayoutList(Some(dispositions.as_mut_slice())) };
        dispositions.truncate(usize::try_from(n).unwrap_or(0));
        let mut etat = [0u8; 256];
        for touche in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_LMENU] {
            etat[usize::from(touche.0)] = 0x80;
        }
        dispositions
            .into_iter()
            .map(|d| {
                let mut tampon = [0u16; 8];
                // SAFETY : la traduction d'une touche par une disposition chargée de la session ; `etat` fait 256 octets,
                // Windows écrit au plus `tampon.len()` caractères et, avec ce drapeau, ne touche pas l'état du clavier.
                let n = unsafe {
                    let balayage = MapVirtualKeyExW(TOUCHE, MAPVK_VK_TO_VSC, Some(d));
                    ToUnicodeEx(TOUCHE, balayage, &etat, &mut tampon, SANS_TOUCHER_L_ETAT, Some(d))
                };
                // Le numéro tient dans les 32 bits du bas (un HKL vaut un nombre, pas une adresse).
                (d.0 as usize as u32, caractere_produit(n, &tampon))
            })
            .collect()
    }

    /// Une chaîne UTF-16 terminée par un zéro, pour Windows.
    pub(super) fn large(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// Écrit une valeur REG_SZ sous HKCU\`cle` (la clé se crée au besoin).
    pub(super) fn ecrire_valeur(cle: &str, nom: &str, texte: &str) -> Result<(), String> {
        let (cle_l, nom_l, texte_l) = (large(cle), large(nom), large(texte));
        let octets = u32::try_from(texte_l.len() * 2).map_err(|_| format!("« {nom} » : valeur trop longue"))?;
        // SAFETY : trois chaînes UTF-16 terminées par un zéro, qui vivent pendant l'appel ; `octets` est la taille du
        // texte, zéro final compris.
        let e = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(cle_l.as_ptr()),
                PCWSTR(nom_l.as_ptr()),
                REG_SZ.0,
                Some(texte_l.as_ptr().cast()),
                octets,
            )
        };
        e.ok().map_err(|erreur| format!("le registre refuse d'écrire « {nom} » ({erreur})"))
    }

    /// Efface HKCU\`cle`, ses sous-clés et ses valeurs ; absente : pas une erreur.
    pub(super) fn effacer_cle(cle: &str) -> Result<(), String> {
        let cle_l = large(cle);
        // SAFETY : une chaîne UTF-16 terminée par un zéro, qui vit pendant l'appel.
        match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(cle_l.as_ptr())) } {
            ERROR_FILE_NOT_FOUND => Ok(()),
            e => e.ok().map_err(|erreur| format!("le registre refuse d'effacer « {cle} » ({erreur})")),
        }
    }

    /// Retire une valeur de HKCU\`cle` ; absente (ou la clé absente) : pas une erreur.
    pub(super) fn supprimer_valeur(cle: &str, nom: &str) -> Result<(), String> {
        let (cle_l, nom_l) = (large(cle), large(nom));
        // SAFETY : deux chaînes UTF-16 terminées par un zéro, qui vivent pendant l'appel.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(cle_l.as_ptr()), PCWSTR(nom_l.as_ptr())) } {
            ERROR_FILE_NOT_FOUND => Ok(()),
            e => e.ok().map_err(|erreur| format!("le registre refuse de retirer « {nom} » ({erreur})")),
        }
    }

    /// Lit une valeur REG_SZ de HKCU\`cle` ; absente (ou la clé absente) : `None`.
    pub(super) fn lire_valeur(cle: &str, nom: &str) -> Result<Option<String>, String> {
        let (cle_l, nom_l) = (large(cle), large(nom));
        let mut tampon = vec![0u16; 260];
        // Trop court, le tampon s'agrandit à la taille que dit Windows ; la valeur peut encore changer entre deux appels.
        for _ in 0..4 {
            let mut octets = u32::try_from(tampon.len() * 2).unwrap_or(u32::MAX);
            // SAFETY : la clé et le nom sont des chaînes UTF-16 terminées par un zéro ; Windows écrit au plus `octets`
            // octets dans le tampon (sinon il rend ERROR_MORE_DATA et la taille qu'il lui faut), zéro final compris.
            let e = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(cle_l.as_ptr()),
                    PCWSTR(nom_l.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(tampon.as_mut_ptr().cast()),
                    Some(&raw mut octets),
                )
            };
            match e {
                ERROR_FILE_NOT_FOUND => return Ok(None),
                ERROR_MORE_DATA => tampon.resize((octets as usize).div_ceil(2) + 1, 0),
                e if e.is_ok() => {
                    let n = (octets as usize / 2).min(tampon.len());
                    let fin = tampon[..n].iter().position(|&c| c == 0).unwrap_or(n);
                    return Ok(Some(String::from_utf16_lossy(&tampon[..fin])));
                }
                e => return Err(format!("le registre ne lit pas « {nom} » ({})", windows::core::Error::from(e))),
            }
        }
        Err(format!("« {nom} » change pendant qu'on la lit"))
    }
}

/// Ailleurs que sous Windows : ni fenêtre d'Edge, ni raccourci, ni registre.
#[cfg(not(windows))]
mod imp {
    pub(super) fn trouver() -> Option<()> {
        None
    }

    pub(super) fn poser_devant(_fenetre: (), _oui: bool) {}

    pub(super) fn basculer() {}

    pub(super) fn montrer() {}

    pub(super) fn demarrer_raccourci() {}

    pub(super) fn ecrire_valeur(_cle: &str, _nom: &str, _texte: &str) -> Result<(), String> {
        Err("le démarrage avec Windows n'existe que sous Windows".into())
    }

    pub(super) fn supprimer_valeur(_cle: &str, _nom: &str) -> Result<(), String> {
        Ok(())
    }

    pub(super) fn effacer_cle(_cle: &str) -> Result<(), String> {
        Ok(())
    }

    pub(super) fn lire_valeur(_cle: &str, _nom: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod essais {
    use std::fs;

    use super::*;

    /// Un dossier temporaire, retiré à la fin de l'essai, même s'il échoue.
    struct Temporaire(PathBuf);

    impl Temporaire {
        fn nouveau(nom: &str) -> Temporaire {
            let d = std::env::temp_dir().join(format!("eve-essai-{nom}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).expect("le dossier temporaire");
            Temporaire(d)
        }

        /// Un chemin sous le dossier (« a/b/c ») ; rien n'est créé.
        fn sous(&self, chemin: &str) -> PathBuf {
            chemin.split('/').fold(self.0.clone(), |p, partie| p.join(partie))
        }
    }

    impl Drop for Temporaire {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn le_lanceur_se_trouve_en_remontant() {
        let t = Temporaire::nouveau("lanceur");
        fs::create_dir_all(t.sous("eve/target/release/deps")).unwrap();
        fs::write(t.sous("eve/Eve.bat"), "@echo off\r\n").unwrap();
        let attendu = Some(t.sous("eve/Eve.bat"));
        // Trois dossiers depuis target\release, quatre depuis l'exécutable d'un essai, deux depuis bin.
        assert_eq!(chercher_lanceur(&t.sous("eve/target/release/eve.exe")), attendu);
        assert_eq!(chercher_lanceur(&t.sous("eve/target/release/deps/eve-0123abcd.exe")), attendu);
        assert_eq!(chercher_lanceur(&t.sous("eve/bin/eve.exe")), attendu);
        // Au-delà de quatre dossiers, on ne cherche plus.
        assert_eq!(chercher_lanceur(&t.sous("eve/a/b/c/d/eve.exe")), None);
        // Le plus proche l'emporte ; un dossier qui porte le nom ne compte pas.
        fs::create_dir_all(t.sous("eve/target/release/Eve.bat")).unwrap();
        fs::write(t.sous("eve/target/Eve.bat"), "").unwrap();
        assert_eq!(chercher_lanceur(&t.sous("eve/target/release/eve.exe")), Some(t.sous("eve/target/Eve.bat")));
    }

    #[test]
    fn la_commande_vise_le_lanceur() {
        let p = Path::new(r"C:\Users\Martin\Toitures Exemple\Eve\Eve.bat");
        assert_eq!(commande(p), r#""C:\Users\Martin\Toitures Exemple\Eve\Eve.bat""#);
        assert_eq!(fichier_vise(&commande(p)), p);
        assert_eq!(fichier_vise(r#"  "C:\a b\Eve.bat" --min"#), Path::new(r"C:\a b\Eve.bat"));
        assert_eq!(fichier_vise(r"C:\a\Eve.bat"), Path::new(r"C:\a\Eve.bat"));
        assert_eq!(fichier_vise(r#""C:\sans fin"#), Path::new(r#""C:\sans fin"#));
    }

    #[test]
    fn le_titre_d_eve() {
        assert!(titre_d_eve(&titre_pour(crate::reglages::nom_affiche())));
        assert!(titre_d_eve("Eve — Adjointe - Microsoft Edge"));
        assert!(!titre_d_eve("Eve - journal (ne pas fermer)"));
        assert!(!titre_d_eve("Boîte de réception - fournisseur@exemple.ca - Outlook"));
        assert!(!titre_d_eve(""));
        // Deux Eve sur un même poste : chacune sa fenêtre, et sa valeur de démarrage avec Windows.
        assert!(!"Eve — Designer - Microsoft Edge".starts_with(&titre_pour("Adjointe")));
        assert!(!"Eve — Adjointe - Microsoft Edge".starts_with(&titre_pour("Designer")));
        assert_eq!(valeur_run_pour("Adjointe"), "Eve (Entrepreneur Général)");
        assert_eq!(valeur_run_pour("Garagiste"), "Eve (Garagiste)");
    }

    #[test]
    fn le_geste_du_raccourci() {
        assert_eq!(geste(true, false), Geste::Reduire);
        assert_eq!(geste(true, true), Geste::Restaurer);
        assert_eq!(geste(false, true), Geste::Restaurer);
        assert_eq!(geste(false, false), Geste::Afficher);
    }

    #[test]
    fn le_caractere_d_une_touche() {
        assert_eq!(caractere_produit(1, &[0x20AC, 0]).as_deref(), Some("€"));
        assert_eq!(caractere_produit(0, &[0x20AC]), None);
        // Une touche morte : le nombre est négatif, le caractère est là.
        assert_eq!(caractere_produit(-1, &[u16::from(b'^')]).as_deref(), Some("^"));
        // Un caractère de contrôle n'écrit rien.
        assert_eq!(caractere_produit(1, &[0x05]), None);
        // Jamais au-delà du tampon.
        assert_eq!(caractere_produit(9, &[u16::from(b'a')]).as_deref(), Some("a"));
    }

    #[test]
    fn l_avertissement_du_clavier() {
        assert_eq!(nom_du_clavier(0x1009_0C0C), "français (Canada) 10090C0C");
        assert_eq!(nom_du_clavier(0xF020_0409), "anglais (États-Unis) F0200409");
        assert_eq!(nom_du_clavier(0x0407_0407), "04070407");
        assert_eq!(avertissement(&[]), None);
        let ligne = avertissement(&[(0x1009_0C0C, "€".into()), (0xF020_0409, "€".into())]).unwrap();
        assert!(ligne.starts_with(
            "Attention : Ctrl+Alt+E (AltGr+E) écrit « € » avec le clavier français (Canada) 10090C0C, « € » avec le \
             clavier anglais (États-Unis) F0200409 ;"
        ));
    }

    #[test]
    fn un_appel_plus_recent_remplace_le_precedent() {
        let relais = Relais::nouveau();
        let (ancien, recent) = (relais.nouveau_tour(), relais.nouveau_tour());
        let mut poses = Vec::new();
        let (pas, delai) = (Duration::from_millis(1), Duration::from_secs(1));
        assert_eq!(relais.reessayer(ancien, delai, pas, || Some(1), |f| poses.push(f)), Issue::Remplace);
        assert_eq!(relais.reessayer(recent, delai, pas, || Some(2), |f| poses.push(f)), Issue::Pose);
        assert_eq!(poses, [2]);
    }

    #[test]
    fn la_fenetre_qui_tarde_et_celle_qui_ne_vient_pas() {
        let relais = Relais::nouveau();
        let (mut essais, mut pose) = (0, None);
        let moi = relais.nouveau_tour();
        let chercher = || {
            essais += 1;
            (essais == 3).then_some("fenêtre")
        };
        let issue =
            relais.reessayer(moi, Duration::from_secs(5), Duration::from_millis(1), chercher, |f| pose = Some(f));
        assert_eq!((issue, essais, pose), (Issue::Pose, 3, Some("fenêtre")));

        let moi = relais.nouveau_tour();
        let debut = Instant::now();
        let issue = relais.reessayer(
            moi,
            Duration::from_millis(30),
            Duration::from_millis(5),
            || None::<()>,
            |_| unreachable!(),
        );
        assert_eq!(issue, Issue::Introuvable);
        assert!(debut.elapsed() >= Duration::from_millis(30));
    }

    /// Les essais sur le poste, un par un : `cargo test --release -- --ignored --nocapture <nom>`.
    #[cfg(windows)]
    mod sur_le_poste {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegDeleteKeyW, RegOpenKeyExW,
        };
        use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GWL_EXSTYLE, GetWindowLongW, IsIconic, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_POPUP,
        };
        use windows::core::{PCWSTR, w};

        use super::*;

        /// La fenêtre est-elle « toujours devant » ? (Une lecture.)
        fn toujours_devant(f: HWND) -> bool {
            // SAFETY : une lecture du style étendu d'une fenêtre.
            let style = unsafe { GetWindowLongW(f, GWL_EXSTYLE) } as u32;
            style & WS_EX_TOPMOST.0 != 0
        }

        /// La vraie fenêtre d'Eve, EN LECTURE SEULE : ni réduite, ni restaurée, ni mise devant.
        #[test]
        #[ignore = "a besoin de la fenêtre d'Eve ouverte sur le poste"]
        fn la_vraie_fenetre_d_eve() {
            let f = imp::trouver().expect("la fenêtre d'Eve est introuvable : Eve doit tourner pour cet essai");
            // SAFETY : une lecture de l'état d'une fenêtre.
            let reduite = unsafe { IsIconic(f) }.as_bool();
            println!("titre : {}", imp::titre(f));
            println!("classe : {} ; programme : {:?}", imp::classe(f), imp::programme(f));
            println!(
                "au premier plan : {} ; réduite : {reduite} ; toujours devant : {}",
                imp::est_devant(f),
                toujours_devant(f)
            );
            assert!(titre_d_eve(&imp::titre(f)));
            assert_eq!(imp::classe(f), CLASSE);
        }

        /// Ce qu'écrit Ctrl+Alt+E avec les dispositions de clavier du poste (une lecture).
        #[test]
        #[ignore = "lit les dispositions de clavier du poste"]
        fn ce_qu_ecrit_ctrl_alt_e() {
            let dispositions = imp::caracteres_ctrl_alt_e();
            for (d, c) in &dispositions {
                println!("clavier {} : Ctrl+Alt+E écrit {c:?}", nom_du_clavier(*d));
            }
            let ecrits: Vec<(u32, String)> = dispositions.iter().filter_map(|(d, c)| Some((*d, c.clone()?))).collect();
            println!("journal : {:?}", avertissement(&ecrits));
            assert!(!dispositions.is_empty());
        }

        /// Ctrl+Alt+E est-il libre ? Enregistré pour le fil de l'essai, retiré aussitôt.
        #[test]
        #[ignore = "enregistre Ctrl+Alt+E un instant"]
        fn le_raccourci_est_libre() {
            // SAFETY : un raccourci du fil courant, retiré par ce même fil.
            let r = unsafe { RegisterHotKey(None, 0x4545, imp::MODIFICATEURS, imp::TOUCHE) };
            if r.is_ok() {
                // SAFETY : le raccourci enregistré ci-dessus, par ce fil.
                let _ = unsafe { UnregisterHotKey(None, 0x4545) };
            }
            println!("Ctrl+Alt+E : {r:?}");
            r.expect("Ctrl+Alt+E est pris (par Eve elle-même, si elle tourne avec ce code)");
        }

        /// « Toujours devant » sur une fenêtre à l'essai, cachée : jamais montrée, jamais activée.
        #[test]
        #[ignore = "crée une fenêtre cachée"]
        fn toujours_devant_sur_une_fenetre_cachee() {
            // SAFETY : une fenêtre « STATIC » de ce fil, sans WS_VISIBLE, détruite à la fin par ce même fil.
            let f = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("essai"),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("la fenêtre d'essai");
            imp::poser_devant(f, true);
            let devant = toujours_devant(f);
            imp::poser_devant(f, false);
            let plus_devant = toujours_devant(f);
            // SAFETY : la fenêtre créée ci-dessus, par ce fil.
            let _ = unsafe { DestroyWindow(f) };
            assert!(devant, "HWND_TOPMOST");
            assert!(!plus_devant, "HWND_NOTOPMOST");
        }

        const PARENT_ESSAI: &str = r"Software\EntrepreneurGeneral";
        const CLE_ESSAI: &str = r"Software\EntrepreneurGeneral\EveEssai";

        fn cle_existe(cle: &str) -> bool {
            let l = imp::large(cle);
            let mut h = HKEY::default();
            // SAFETY : une chaîne UTF-16 terminée par un zéro ; la clé ouverte se referme aussitôt.
            unsafe {
                let ouverte = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(l.as_ptr()), None, KEY_READ, &raw mut h).is_ok();
                if ouverte {
                    let _ = RegCloseKey(h);
                }
                ouverte
            }
        }

        /// Retire la clé d'essai à la fin, même si l'essai échoue — et son parent s'il est né de l'essai. RegDeleteKeyW ne
        /// retire qu'une clé sans sous-clé.
        struct CleEssai {
            parent_cree: bool,
        }

        impl Drop for CleEssai {
            fn drop(&mut self) {
                let cles = if self.parent_cree { vec![CLE_ESSAI, PARENT_ESSAI] } else { vec![CLE_ESSAI] };
                for cle in cles {
                    let l = imp::large(cle);
                    // SAFETY : une chaîne UTF-16 terminée par un zéro, qui vit pendant l'appel.
                    let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(l.as_ptr())) };
                }
            }
        }

        /// Le démarrage avec Windows, sous une clé d'essai (jamais sous Run) : écrire, lire, retirer, retirer encore.
        #[test]
        #[ignore = "écrit dans le registre (HKCU\\Software\\EntrepreneurGeneral\\EveEssai, retirée à la fin)"]
        fn le_demarrage_sous_une_cle_d_essai() {
            let _garde = CleEssai { parent_cree: !cle_existe(PARENT_ESSAI) };
            let nom = valeur_run();
            imp::supprimer_valeur(CLE_ESSAI, &nom).unwrap();
            assert!(!inscrit(CLE_ESSAI));

            let lanceur = inscrire(CLE_ESSAI, true).unwrap().expect("le lanceur inscrit");
            let valeur = imp::lire_valeur(CLE_ESSAI, &nom).unwrap();
            println!("« {nom} » = {valeur:?}");
            assert!(lanceur.is_file() && lanceur.ends_with(LANCEUR));
            assert_eq!(valeur, Some(commande(&lanceur)));
            assert!(inscrit(CLE_ESSAI));

            // Une valeur qui vise un fichier disparu : pas inscrit.
            imp::ecrire_valeur(CLE_ESSAI, &nom, r#""C:\nulle part\Eve.bat""#).unwrap();
            assert!(!inscrit(CLE_ESSAI));

            // Retirer, puis retirer encore : absente, ce n'est pas une erreur.
            assert_eq!(inscrire(CLE_ESSAI, false).unwrap(), None);
            assert_eq!(imp::lire_valeur(CLE_ESSAI, &nom).unwrap(), None);
            assert!(!inscrit(CLE_ESSAI));
            assert_eq!(inscrire(CLE_ESSAI, false).unwrap(), None);
        }

        /// La vraie valeur Run (une lecture) : absente tant que personne n'a demandé le démarrage avec Windows.
        #[test]
        #[ignore = "lit la clé Run de l'utilisateur"]
        fn la_vraie_valeur_run() {
            let nom = valeur_run();
            let valeur = imp::lire_valeur(CLE_RUN, &nom).expect("la clé Run se lit");
            let actif = demarrage_windows_actif();
            println!("« {nom} » = {valeur:?} ; démarrage avec Windows : {actif}");
            assert_eq!(actif, valeur.is_some_and(|c| fichier_vise(&c).is_file()));
        }
    }
}
