//! La fenêtre native d'Eve (le lot 6) : egui, dans le programme même, dessinée par la carte graphique (OpenGL) — plus de
//! page dans Edge. Elle lit l'état que publie le chef (le même JSON que la page, [`Ecran::abonner`]) et lui rend les
//! gestes de la personne (`Evt::Ui`, les mêmes actions que la page) : le chef ne voit pas la différence.
//!
//! Deux colonnes, dans les couleurs de Constructo AI (celles de l'Eve de Constructo AI, en sombre) : à gauche la
//! conversation (l'orbe, le fil, la saisie, l'accord), à droite le centre de commande ([`crate::centre`]), que le
//! bouton de la barre replie et qui se replie de lui-même dans une fenêtre étroite. La barre porte la marque : le logo
//! de Constructo AI, « EVE de Constructo AI », le spécialiste et la personne ; elle est aussi la barre de titre de la
//! fenêtre ([`crate::cadre`]) : sa bande du haut, à la place de celle de Windows, porte le site de Constructo AI et
//! les boutons de la fenêtre. L'icône de la fenêtre est le logo. Les icônes sont celles de Windows (Segoe Fluent Icons). La
//! fenêtre porte le titre de la fenêtre d'Edge : le raccourci Ctrl+Alt+E et « toujours devant » la retrouvent de même
//! (`fenetre.rs`). L'inventaire de ce qui reste : `docs\fenetre-native.md`.
//!
//! [`Ecran::abonner`]: crate::ecran::Ecran::abonner

pub(crate) mod graphiques;
pub(crate) mod pieces;
pub(crate) mod portrait;
pub(crate) mod profils;
pub(crate) mod tableau;
pub(crate) mod theme;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, RichText, Sense, Stroke, vec2,
};
use serde_json::Value;

use crate::ambiance::{self, Ambiance, Moment, Veille};
use crate::cadre;
use crate::cartes::{Cartes, Geste};
use crate::centre::{self, Centre};
use crate::chef::Evt;
use crate::ecran::{Abonnement, Presence};
use crate::historique_ui::{self, Historique, Sortie};
use crate::journal;
use crate::meteo::{self, Bulletin, Meteo};
use crate::musique::Musique;
use crate::orbe::Orbe;
use crate::reseau::{CURSEUR, Evenement, Reseau};
use crate::services::{Releve, Services};
use crate::stripe::{Montants, Stripe};

/// Les couleurs de l'Eve de Constructo AI, thème sombre (sa page, `jarvis.html`, ses variables CSS).
pub mod couleurs {
    use eframe::egui::Color32;

    pub const FOND: Color32 = Color32::from_rgb(0x0F, 0x19, 0x23);
    pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x21, 0x2E);
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x1B, 0x28, 0x38);
    pub const ENCRE: Color32 = Color32::from_rgb(0xE5, 0xE9, 0xF0);
    pub const ENCRE_2: Color32 = Color32::from_rgb(0xAA, 0xB6, 0xC6);
    pub const ENCRE_3: Color32 = Color32::from_rgb(0x8F, 0x9C, 0xAE);
    pub const FILET: Color32 = Color32::from_rgb(0x26, 0x34, 0x46);
    pub const FILET_FORT: Color32 = Color32::from_rgb(0x3A, 0x4A, 0x5F);
    pub const ACCENT: Color32 = Color32::from_rgb(0x58, 0xA6, 0xFF);
    pub const CRIT: Color32 = Color32::from_rgb(0xF0, 0x7A, 0x7A);
    pub const OUI: Color32 = Color32::from_rgb(0x7D, 0xDB, 0x7D);
    pub const AMBRE: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
    pub const ALERTE_FOND: Color32 = Color32::from_rgb(0x2A, 0x25, 0x14);
    pub const ALERTE_BORD: Color32 = Color32::from_rgb(0x4A, 0x40, 0x19);
    pub const PILULE: Color32 = Color32::from_rgb(0x2A, 0x3A, 0x4F);
    /// Les bulles de la personne : l'accent à 18 % sur la surface, et à 35 % pour leur bord.
    pub const BULLE_TOI: Color32 = Color32::from_rgb(0x22, 0x39, 0x54);
    pub const BULLE_TOI_BORD: Color32 = Color32::from_rgb(0x2D, 0x4F, 0x77);
    /// Le texte d'un bouton plein (l'accent, le vert) : sombre, pour se lire.
    pub const SUR_PLEIN: Color32 = Color32::from_rgb(0x0B, 0x12, 0x20);
    /// La barre, marine dans les deux thèmes : son dégradé, son texte, son texte clair.
    pub const BARRE_HAUT: Color32 = Color32::from_rgb(0x00, 0x2B, 0x6B);
    pub const BARRE_BAS: Color32 = Color32::from_rgb(0x00, 0x20, 0x50);
    pub const BARRE_TEXTE: Color32 = Color32::from_rgb(0xC7, 0xD4, 0xE8);
    pub const BARRE_CLAIR: Color32 = Color32::from_rgb(0xE6, 0xEE, 0xF8);
    /// Le rouge de Windows 11 au survol du bouton qui ferme la fenêtre.
    pub const FERMER: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);
    /// Les pastilles d'état de la barre.
    pub const PASTILLE_OK: Color32 = Color32::from_rgb(0x34, 0xC7, 0x59);
    pub const PASTILLE_NEUTRE: Color32 = Color32::from_rgb(0x6B, 0x77, 0x85);
    /// L'orbe : le dégradé de son cœur, du reflet au bord.
    pub const ORBE: [Color32; 4] = [
        Color32::from_rgb(0x5B, 0x8F, 0xD0),
        Color32::from_rgb(0x1F, 0x4C, 0x8A),
        Color32::from_rgb(0x12, 0x30, 0x5C),
        Color32::from_rgb(0x0B, 0x1B, 0x33),
    ];
}

/// Les couleurs des panneaux qui s'ouvrent par-dessus la fenêtre — les réglages, l'historique, le menu des profils — et
/// de ce qu'on y touche : celles du thème sombre, ou celles du thème « Constructo AI », blanches comme les cartes de
/// son tableau de bord. Un panneau prend celles de son `Ui` ([`panneau`]) ; dans le thème clair, la fenêtre lui donne
/// ses visuels ([`theme::visuels_du_panneau`]).
#[derive(Clone, Copy)]
pub(crate) struct Panneau {
    /// Le panneau : son fond, son filet, son ombre proche et sa large.
    pub(crate) fond: Color32,
    pub(crate) filet: Color32,
    pub(crate) ombres: [egui::Shadow; 2],
    /// Un groupe de rangées, une touche du clavier ; la piste d'un choix en segments et sa case choisie ; un survol.
    pub(crate) groupe: Color32,
    pub(crate) piste: Color32,
    pub(crate) choisi: Color32,
    pub(crate) survol: Color32,
    /// Le texte, le secondaire, le discret.
    pub(crate) encre: Color32,
    pub(crate) encre_2: Color32,
    pub(crate) encre_3: Color32,
    /// Un filet plus marqué (un bouton cerné, un interrupteur éteint, un champ) ; l'accent.
    pub(crate) filet_fort: Color32,
    pub(crate) accent: Color32,
    /// Le texte d'un bouton plein ; le rouge d'une erreur et de « Quitter Eve » ; le fond d'un champ.
    pub(crate) sur_plein: Color32,
    pub(crate) crit: Color32,
    pub(crate) champ: Color32,
}

/// Le thème sombre : les couleurs de l'Eve de Constructo AI ([`couleurs`]), l'ombre de ses panneaux
/// (`--ombre-panneau` : une proche et nette, une large et douce).
pub(crate) const PANNEAU_SOMBRE: Panneau = Panneau {
    fond: couleurs::SURFACE,
    filet: couleurs::FILET,
    ombres: [
        egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(100) },
        egui::Shadow { offset: [0, 16], blur: 44, spread: 0, color: Color32::from_black_alpha(150) },
    ],
    groupe: couleurs::SURFACE_2,
    piste: couleurs::SURFACE_2,
    choisi: couleurs::PILULE,
    survol: Color32::from_rgba_premultiplied(12, 12, 12, 12),
    encre: couleurs::ENCRE,
    encre_2: couleurs::ENCRE_2,
    encre_3: couleurs::ENCRE_3,
    filet_fort: couleurs::FILET_FORT,
    accent: couleurs::ACCENT,
    sur_plein: couleurs::SUR_PLEIN,
    crit: couleurs::CRIT,
    champ: couleurs::FOND,
};

/// Le thème « Constructo AI » : blanc, le filet froid de ses cartes et leurs ombres teintées de marine
/// ([`theme::ombre_marine`]), les gris d'ardoise de ses tableaux de bord (le groupe : `slate-50`, la piste : `-100`, le
/// filet fort : `-300`, le discret : `-500`), son bleu primaire.
pub(crate) const PANNEAU_CLAIR: Panneau = Panneau {
    fond: Color32::WHITE,
    filet: theme::erp::FILET_DE_CARTE,
    ombres: [
        egui::Shadow {
            offset: [0, 2],
            blur: 6,
            spread: 0,
            color: Color32::from_rgba_unmultiplied_const(15, 35, 80, 22),
        },
        egui::Shadow {
            offset: [0, 16],
            blur: 44,
            spread: 0,
            color: Color32::from_rgba_unmultiplied_const(15, 35, 80, 52),
        },
    ],
    groupe: Color32::from_rgb(0xF8, 0xFA, 0xFC),
    piste: Color32::from_rgb(0xF1, 0xF5, 0xF9),
    choisi: Color32::WHITE,
    survol: Color32::from_black_alpha(10),
    encre: theme::erp::TITRE,
    encre_2: theme::erp::ARDOISE_2,
    encre_3: Color32::from_rgb(0x64, 0x74, 0x8B),
    filet_fort: Color32::from_rgb(0xCB, 0xD5, 0xE1),
    accent: theme::erp::PRIMAIRE,
    sur_plein: Color32::WHITE,
    crit: theme::erp::DANGER_ENCRE,
    champ: Color32::WHITE,
};

/// Les couleurs d'un panneau dans ce `Ui` : claires dans un `Ui` aux visuels clairs.
pub(crate) fn panneau(ui: &egui::Ui) -> &'static Panneau {
    if ui.visuals().dark_mode { &PANNEAU_SOMBRE } else { &PANNEAU_CLAIR }
}

/// Les icônes de Windows : Segoe Fluent Icons (Windows 11), ou Segoe MDL2 Assets (Windows 10), qui a les mêmes
/// points de code.
pub mod icones {
    pub const MICRO: char = '\u{E720}';
    pub const ENVOYER: char = '\u{E724}';
    pub const ARRETER: char = '\u{E71A}';
    pub const NOUVELLE: char = '\u{E710}';
    pub const VOLET: char = '\u{E89F}';
    pub const ALERTE: char = '\u{E7BA}';
    pub const CLOCHE: char = '\u{EA8F}';
    pub const OUI: char = '\u{E73E}';
    pub const NON: char = '\u{E711}';
    pub const REGLAGES: char = '\u{E713}';
    pub const GLOBE: char = '\u{E774}';
    pub const PAGE_WEB: char = '\u{E8A1}';
    pub const CALENDRIER: char = '\u{E787}';
    pub const COURRIEL: char = '\u{E715}';
    pub const PIECE: char = '\u{E723}';
    pub const COPIER: char = '\u{E8C8}';
    /// La flèche vers le bas (Down) : le bas de la conversation.
    pub const EN_BAS: char = '\u{E74B}';
    /// Les boutons de la fenêtre : les glyphes de ceux de Windows (ChromeMinimize, ChromeMaximize, ChromeRestore,
    /// ChromeClose).
    pub const REDUIRE: char = '\u{E921}';
    pub const AGRANDIR: char = '\u{E922}';
    pub const RESTAURER: char = '\u{E923}';
    pub const FERMER: char = '\u{E8BB}';
}

/// Les couleurs du tableau HTML de l'Assistante famille (`centre-de-commande.html`) : le centre de commande les prend,
/// comme sa police à chasse fixe.
pub mod hud {
    use eframe::egui::Color32;

    pub const CYAN: Color32 = Color32::from_rgb(0x00, 0xE5, 0xFF);
    pub const VERT: Color32 = Color32::from_rgb(0x7C, 0xFF, 0x6B);
    pub const ROUGE: Color32 = Color32::from_rgb(0xFF, 0x3B, 0x4E);
    pub const ROUGE_PALE: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x7A);
    pub const AMBRE: Color32 = Color32::from_rgb(0xFF, 0xB3, 0x47);
    pub const JAUNE: Color32 = Color32::from_rgb(0xFF, 0xD2, 0x3F);
    pub const TEXTE: Color32 = Color32::from_rgb(0xD9, 0xF3, 0xFF);
    pub const TEXTE_CHAUD: Color32 = Color32::from_rgb(0xFF, 0xEF, 0xD6);
    pub const GRIS: Color32 = Color32::from_rgb(0x5F, 0x8A, 0xA8);
    /// La ligne d'état et l'écran de veille ; l'horloge ; la légende et l'aide des touches.
    pub const BLANC: Color32 = Color32::from_rgb(0xE8, 0xFB, 0xFF);
    pub const HORLOGE: Color32 = Color32::from_rgb(0x7F, 0xB4, 0xD4);
    pub const SOMBRE: Color32 = Color32::from_rgb(0x48, 0x6B, 0x85);
    /// Le fond d'un élément (rgba(8, 18, 36, .55), prémultiplié) ; le panneau de la conversation et son bord.
    pub const PANNEAU: Color32 = Color32::from_rgba_premultiplied(4, 10, 20, 140);
    pub const FLUX: Color32 = Color32::from_rgb(0x06, 0x0F, 0x1C);
    pub const BORD: Color32 = Color32::from_rgb(0x10, 0x2E, 0x44);
}

/// Le logo de Constructo AI (celui de la barre de l'ERP), dessiné en blanc.
const LOGO: &[u8] = include_bytes!("../page/constructo-ai.png");
/// Les familles de polices : Segoe UI Bold pour « EVE », Semibold pour ce qui se détache, les icônes de Windows.
pub(crate) const GRAS: &str = "gras";
pub(crate) const SEMI: &str = "semi";
pub(crate) const ICONES: &str = "icones";
/// En dessous de cette largeur, le centre de commande se replie de lui-même : il lui faut la conversation à droite, et
/// la place du réseau, de ses panneaux et de la décision du jour.
const TABLEAU: f32 = 1100.0;
/// Le temps qu'une impulsion met à traverser le réseau, d'Eve à sa destination : l'accord de l'arrivée sonne alors.
const ARRIVEE: f64 = 0.9;
/// La barre : sa bande du haut, à la hauteur de la barre de titre de Windows qu'elle remplace (et de ses boutons), puis
/// sa rangée ; un bouton de la fenêtre a la largeur de ceux de Windows 11.
pub(crate) const BANDE: f32 = 32.0;
pub(crate) const RANGEE: f32 = 58.0;
/// La première ligne du titre commence ce tant au-dessus du haut du logo : ce que garde au-dessus de ses capitales la
/// police de « EVE » (17 points), pour que leur haut soit celui du logo (mesuré sur une capture).
const AU_DESSUS_DES_CAPITALES: f32 = 7.0;
/// La seconde ligne commence ce tant au-dessus du bas du logo : sa hauteur jusqu'à la ligne de base de ses lettres
/// (12 points), pour que leur bas soit celui du logo (mesuré sur une capture).
const SOUS_LA_LIGNE: f32 = 13.0;
const BOUTON_DE_FENETRE: f32 = 46.0;

/// Le contexte d'egui, pour que la fenêtre se ferme quand Eve s'arrête d'elle-même.
static CONTEXTE: OnceLock<egui::Context> = OnceLock::new();

/// Ouvre la fenêtre et rend la main quand elle se ferme ; `false` si elle n'a pas pu s'ouvrir (pas d'OpenGL).
/// `dossier` : le dossier de travail, où le centre de commande lit ses panneaux.
pub fn lancer(abonnement: Abonnement, rapport: Sender<Evt>, titre: &str, dossier: Option<PathBuf>) -> bool {
    let (taille, position) = place_initiale();
    let mut vue =
        egui::ViewportBuilder::default().with_title(titre).with_inner_size(taille).with_min_inner_size([400.0, 540.0]);
    if let Some(p) = position {
        vue = vue.with_position(p);
    }
    if let Some(i) = icone_de_fenetre() {
        vue = vue.with_icon(Arc::new(i));
    }
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let creer: eframe::AppCreator<'_> =
        Box::new(move |cc| Ok(Box::new(Fenetre::nouvelle(cc, abonnement, rapport, dossier))));
    match eframe::run_native(titre, options, creer) {
        Ok(()) => true,
        Err(e) => {
            journal::ecrire(&format!("La fenêtre native ne s'ouvre pas ({e}) : la fenêtre d'Edge la remplace."));
            false
        }
    }
}

/// Le temps qu'une image de la fenêtre met à se voir : trois trames de l'écran. Mesuré le 2026-10-25 dans la vraie
/// fenêtre, à 60 Hz, avec la cadence de `ui` : 32 ms de son calcul à la trame où le bureau la montre (deux trames, à la
/// milliseconde près) ; puis l'écran la balaie jusqu'au visage et la traite, ce qu'aucune capture ne voit. La voix
/// d'Eve s'y lit d'autant plus loin ([`crate::voix::regler_le_retard_de_l_ecran`]) : mesurée de bout en bout (l'image
/// capturée contre le son qui sort du poste), la bouche vue suivait le son de 5 ms ; avec trois trames, elle le précède
/// de 45 ms (de 40 à 55 selon la phrase ; 38 avec deux trames) — comme une vraie bouche, qui se place avant le son. La
/// trame est celle de l'écran (un écran à 144 Hz montre ses images plus vite) ; sans elle, celle d'un écran à 60 Hz.
fn retard_de_l_ecran() -> Duration {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmGetCompositionTimingInfo};
        let mut t = DWM_TIMING_INFO { cbSize: size_of::<DWM_TIMING_INFO>() as u32, ..Default::default() };
        // SAFETY : la structure est à nous, sa taille dite ; la fenêtre nulle : la composition de tout le bureau.
        let lue = unsafe { DwmGetCompositionTimingInfo(windows::Win32::Foundation::HWND::default(), &mut t) };
        let frequence = { t.rateRefresh };
        if lue.is_ok() && frequence.uiNumerator > 0 && frequence.uiDenominator > 0 {
            let trame = f64::from(frequence.uiDenominator) / f64::from(frequence.uiNumerator);
            return Duration::from_secs_f64(3.0 * trame.clamp(1.0 / 360.0, 1.0 / 24.0));
        }
    }
    Duration::from_secs_f64(3.0 / 60.0)
}

/// Eve s'arrête d'elle-même : sa fenêtre se ferme avec elle.
pub fn fermer() {
    if let Some(ctx) = CONTEXTE.get() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Comment le fil se dessine : en bulles (la conversation seule, le centre replié), ou comme le flux des entrées et
/// sorties du tableau HTML, dont il prend la place (le centre déplié) ; ou en bulles claires, dans le thème
/// « Constructo AI » ([`theme`]).
#[derive(Clone, Copy, PartialEq)]
enum Style {
    Bulles,
    Flux,
    Claire,
}

pub(crate) struct Fenetre {
    /// Le dernier état publié par le chef, pas encore lu (un fil le dépose et réveille la fenêtre).
    dernier: Arc<Mutex<Option<String>>>,
    etat: Value,
    rapport: Sender<Evt>,
    saisie: String,
    /// La demande reprise par ↑ dans la saisie : combien de demandes en arrière (1 : la dernière), et son texte — elle
    /// ne vaut plus dès qu'on la change ([`Fenetre::champ_de_saisie`]).
    reprise: Option<(usize, String)>,
    /// La hauteur du champ de la saisie à la dernière image : sa rangée y centre ses boutons.
    hauteur_de_la_saisie: f32,
    /// Le bouton du bas du fil vient d'être cliqué : le fil y redescend à l'image suivante.
    au_bas_du_fil: bool,
    /// La note du tableau de bord qu'on écrit (ou qu'on vient de refermer : sa carte garde son texte le temps que le
    /// chef le publie) ; la carte épinglée qu'on vient voir de la conversation (son id, depuis quand, si elle a déjà
    /// défilé sous les yeux).
    note: Option<tableau::NoteOuverte>,
    /// Ce que la fenêtre a dit au chef d'elle-même : devant (la personne la regarde) ou non — le chef ne montre une
    /// notification de Windows que si elle ne l'est pas.
    devant_dit: Option<bool>,
    epingle_a_montrer: Option<(String, f64, bool)>,
    /// Le portrait d'Eve (le thème « Constructo AI »), à la place de l'orbe quand il y en a un.
    portrait: portrait::Portrait,
    /// Le numéro de la dernière phrase mal entendue mise dans la saisie (`a_corriger` de l'état), et la saisie qui
    /// doit prendre le clavier à la prochaine image.
    corrigee: u64,
    saisie_au_clavier: bool,
    /// Le micro ou l'orbe sont tenus ; l'orbe l'est à cette image ; la barre d'espace l'est (hors de la saisie).
    parle: bool,
    orbe_tenue: bool,
    espace: bool,
    /// L'orbe vivante de l'Eve de Constructo AI (ses shaders), et la hauteur de la fenêtre, qui borne sa taille.
    orbe: Orbe,
    hauteur: f32,
    /// La fermeture est déjà dite au chef.
    fermee: bool,
    /// Le centre de commande est déplié ; la fenêtre est assez large pour lui ; le mode propre (H : le réseau seul,
    /// sans ses panneaux).
    centre_ouvert: bool,
    large: bool,
    propre: bool,
    centre: Centre,
    /// Les cartes des réponses d'Eve (leurs aperçus lus, leurs textures), et les dossiers d'où leurs aperçus se lisent
    /// (ceux des profils, que publie le chef : [`profils`]).
    cartes: Cartes,
    dossiers_permis: Vec<PathBuf>,
    /// Le numéro du dernier outil vu ; la dernière phrase de la personne que le réseau a entendue (pour le sujet) ; la
    /// dernière lecture de `sujet.js`.
    outils_vus: u64,
    entendu: String,
    sujet_lu: f64,
    /// La musique et les sons du tableau (`None` sans sortie audio) ; leurs touches (A, S) ; les accords d'arrivée à
    /// venir (une impulsion met un peu moins d'une seconde à traverser le réseau).
    musique: Option<Musique>,
    /// L'ambiance du tableau (la saison, la fête, le balayage, la nuit, l'écran de veille) ; la météo (son fil) et son
    /// dernier bulletin ; les personnes et les mots de l'école déjà donnés au réseau ; la cueillette en cours (ses
    /// notes sonnent quand elle commence).
    ambiance: Ambiance,
    meteo: Option<Meteo>,
    bulletin: Option<Bulletin>,
    /// Les services en ligne et Stripe (leurs suivis, dans le thème « Constructo AI » seulement : son tableau de bord
    /// est le seul à les montrer), et leurs derniers relevés, relus une fois par seconde.
    services: Option<Services>,
    releve: Option<Releve>,
    stripe: Option<Stripe>,
    montants: Option<Result<Montants, String>>,
    personnes_vues: Vec<crate::reseau::Personne>,
    motifs_vus: Vec<String>,
    cueillette: bool,
    musique_allumee: bool,
    sons_allumes: bool,
    arrivees: VecDeque<f64>,
    /// Les réglages : ouverts ou non ; le bouton de la barre qui les ouvre (un clic dessus n'est pas un clic dehors) ;
    /// les valeurs changées ici, tenues quatre secondes le temps que le chef les publie ; le prénom qu'on tape, et le
    /// dernier que le chef a publié.
    pub(crate) reglages_ouverts: bool,
    bouton_reglages: egui::Rect,
    /// L'historique des conversations, ouvert ou non (un panneau neuf à chaque ouverture) ; son bouton de la barre ; la
    /// place du fil, qu'il recouvre.
    historique: Option<Historique>,
    bouton_historique: egui::Rect,
    place_du_fil: egui::Rect,
    tenus: HashMap<String, (bool, f64)>,
    qui_saisi: String,
    qui_vu: String,
    /// La barre est la barre de titre de la fenêtre ([`cadre`]) ; ce qui s'y clique dans egui, et les boutons de la
    /// fenêtre, à l'image d'avant : décrits à Windows.
    cadre: bool,
    zones_de_la_barre: Vec<egui::Rect>,
    boutons_de_fenetre: Vec<(cadre::Bouton, egui::Rect)>,
    /// Le réseau des actions, et s'il s'animait à la dernière image (la fenêtre redessine alors plus souvent).
    reseau: Reseau,
    anime: bool,
    logo: Option<egui::TextureHandle>,
    /// Dès sa première image, et tant qu'elle vit, la fenêtre compte parmi les fenêtres ouvertes : le chef ne se croit
    /// pas seul, et la marque de santé d'une nouvelle version sait la fenêtre ouverte.
    presence: Presence,
}

impl Fenetre {
    pub(crate) fn nouvelle(
        cc: &eframe::CreationContext<'_>,
        abonnement: Abonnement,
        rapport: Sender<Evt>,
        dossier: Option<PathBuf>,
    ) -> Fenetre {
        let ctx = &cc.egui_ctx;
        let _ = CONTEXTE.set(ctx.clone());
        crate::voix::regler_le_retard_de_l_ecran(retard_de_l_ecran());
        #[cfg(windows)]
        let cadre = barre_de_titre(cc);
        #[cfg(not(windows))]
        let cadre = false;
        polices(ctx);
        apparence(ctx);
        let Abonnement { initial, recu, presence } = abonnement;
        let dernier = Arc::new(Mutex::new(Some(initial)));
        let (d, c) = (dernier.clone(), ctx.clone());
        thread::spawn(move || {
            for etat in recu {
                if let Ok(mut x) = d.lock() {
                    *x = Some(etat);
                }
                c.request_repaint();
            }
        });
        Fenetre {
            dernier,
            etat: Value::Null,
            rapport,
            saisie: String::new(),
            reprise: None,
            hauteur_de_la_saisie: 0.0,
            au_bas_du_fil: false,
            note: None,
            devant_dit: None,
            epingle_a_montrer: None,
            portrait: portrait::Portrait::nouveau(crate::reglages::dossier_local()),
            corrigee: 0,
            saisie_au_clavier: false,
            parle: false,
            orbe_tenue: false,
            orbe: Orbe::nouvelle(),
            hauteur: 800.0,
            espace: false,
            fermee: false,
            centre_ouvert: true,
            large: true,
            propre: false,
            reseau: Reseau::nouveau(dossier.as_deref().unwrap_or(Path::new(""))),
            anime: false,
            cartes: Cartes::nouvelles(dossier.clone(), crate::reglages::dossier_local()),
            dossiers_permis: dossier.iter().cloned().collect(),
            centre: Centre::nouveau(dossier, ctx.clone()),
            outils_vus: 0,
            entendu: String::new(),
            sujet_lu: f64::NEG_INFINITY,
            musique: Musique::nouvelle(),
            ambiance: Ambiance::nouvelle(),
            meteo: None,
            bulletin: None,
            services: None,
            releve: None,
            stripe: None,
            montants: None,
            personnes_vues: Vec::new(),
            motifs_vus: Vec::new(),
            cueillette: false,
            musique_allumee: true,
            sons_allumes: true,
            arrivees: VecDeque::new(),
            reglages_ouverts: false,
            bouton_reglages: egui::Rect::NOTHING,
            historique: None,
            bouton_historique: egui::Rect::NOTHING,
            place_du_fil: egui::Rect::NOTHING,
            tenus: HashMap::new(),
            qui_saisi: String::new(),
            qui_vu: String::new(),
            cadre,
            zones_de_la_barre: Vec::new(),
            boutons_de_fenetre: Vec::new(),
            logo: logo_blanc(235).map(|img| ctx.load_texture("logo-constructo-ai", img, egui::TextureOptions::LINEAR)),
            presence,
        }
    }

    /// Un geste de la personne, au chef (comme un `POST /action` de la page).
    fn geste(&self, action: &str, valeur: Value) {
        let _ = self.rapport.send(Evt::Ui { action: action.to_string(), valeur });
    }

    fn champ(&self, cle: &str) -> &str {
        self.etat.get(cle).and_then(Value::as_str).unwrap_or("")
    }

    /// Les outils que l'état apporte et que la fenêtre n'a pas encore vus : chacun part dans le réseau (une impulsion
    /// vers sa destination, puis son retour). `temps` : l'horloge du réseau.
    fn suivre_les_outils(&mut self, temps: f64) {
        for o in self.etat.get("outils").and_then(Value::as_array).into_iter().flatten() {
            let n = o.get("n").and_then(Value::as_u64).unwrap_or(0);
            if n <= self.outils_vus {
                continue;
            }
            self.outils_vus = n;
            let texte = |cle: &str| o.get(cle).and_then(Value::as_str).unwrap_or("").to_string();
            let oui = |cle: &str| o.get(cle).and_then(Value::as_bool).unwrap_or(false);
            let (id, fin, erreur) = (texte("id"), oui("fin"), oui("erreur"));
            // Le réseau prend la cible de l'outil (le dossier de Grep, pas son motif) ; un état d'avant n'a que le détail.
            let cible = o.get("cible").and_then(Value::as_str).map_or_else(|| texte("detail"), str::to_string);
            let evenement = Evenement { id, outil: texte("outil"), detail: cible, fin, erreur };
            self.reseau.recevoir(&evenement, temps);
            self.ambiance.signal(temps);
            // Les notes du tableau : deux qui montent quand l'outil part, une qui se pose quand il revient ; l'accord
            // quand l'impulsion arrive au bout.
            if let Some(m) = self.musique.as_mut() {
                if fin {
                    m.retour(erreur);
                } else {
                    m.aller();
                }
                self.arrivees.push_back(temps + ARRIVEE);
            }
        }
        // Ce que la personne vient de dire fait penser le réseau à une image (la dictée du tableau, que l'oreille d'Eve
        // remplace) ; une ligne d'une séance d'avant, non.
        let dite = self
            .etat
            .get("fil")
            .and_then(Value::as_array)
            .and_then(|f| f.iter().rev().find(|l| l["de"].as_str() == Some("toi")))
            .filter(|l| !l["ancien"].as_bool().unwrap_or(false))
            .map(|l| (l["heure"].as_str().unwrap_or("").to_string(), l["texte"].as_str().unwrap_or("").to_string()));
        if let Some((heure, texte)) = dite {
            let cle = format!("{heure} {texte}");
            if cle != self.entendu {
                self.reseau.entendu(&texte, temps);
                self.ambiance.signal(temps);
                self.entendu = cle;
            }
        }
    }

    /// La barre, tout en haut de la fenêtre. Sa bande du haut remplace la barre de titre de Windows : le site de
    /// Constructo AI au-dessus du logo, les boutons de la fenêtre à droite ([`bande_du_haut`]). Sa rangée : le ruban, le
    /// logo, « EVE de Constructo AI », le spécialiste et la personne ; à droite, les pastilles d'état, le bouton du
    /// centre de commande et celui des réglages. Ce qui s'y clique est noté : Windows le laisse à egui, le reste de la
    /// barre déplace la fenêtre ([`cadre`]).
    fn barre(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        degrade(ui.painter(), rect, couleurs::BARRE_HAUT, couleurs::BARRE_BAS);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, Color32::from_black_alpha(90)));
        self.zones_de_la_barre.clear();
        let haut = if self.cadre { BANDE } else { 0.0 };
        let rangee = egui::Rect::from_min_max(rect.left_top() + vec2(0.0, haut), rect.right_bottom());
        let mut logo = rect.left() + 26.0;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rangee), |ui| self.rangee_de_la_barre(ui, &mut logo));
        if self.cadre {
            let bande = egui::Rect::from_min_size(rect.left_top(), vec2(rect.width(), BANDE));
            let (boutons, site) = bande_du_haut(ui, bande, logo);
            self.boutons_de_fenetre = boutons;
            // Le site se clique : Windows le laisse à egui (le reste de la bande déplace la fenêtre).
            self.zones_de_la_barre.push(site.rect);
            if site.clicked() {
                self.geste("site", Value::Null);
            }
        }
    }

    /// La rangée de la barre, sous sa bande ; `logo` : où commence le logo (le site, au-dessus, s'y aligne).
    fn rangee_de_la_barre(&mut self, ui: &mut egui::Ui, logo: &mut f32) {
        ui.horizontal_centered(|ui| {
            ui.add_space(16.0);
            let (ruban, _) = ui.allocate_exact_size(vec2(4.0, 30.0), Sense::hover());
            ui.painter().rect_filled(ruban, 2.0, couleurs::ACCENT);
            ui.add_space(6.0);
            let centre = ui.max_rect().center().y;
            let image = match &self.logo {
                Some(t) => ui.add(egui::Image::new(t).fit_to_exact_size(vec2(28.0, 28.0))).rect,
                None => egui::Rect::from_center_size(egui::pos2(ui.cursor().left() + 14.0, centre), vec2(28.0, 28.0)),
            };
            *logo = image.left();
            ui.add_space(8.0);
            // Deux lignes serrées contre le logo, comme l'en-tête de l'ERP (« Constructo AI » sur « ERP Construction ») :
            // le haut de « EVE » au haut du logo, le bas du profil et de la personne à son bas. Chaque ligne se dessine
            // à sa place, sans pousser l'autre ; le bloc prend ensuite sa largeur dans la barre.
            let x = ui.cursor().left();
            let haut =
                egui::Rect::from_min_size(egui::pos2(x, image.top() - AU_DESSUS_DES_CAPITALES), vec2(420.0, 24.0));
            let mut nom = ui.new_child(egui::UiBuilder::new().max_rect(haut).layout(Layout::left_to_right(Align::Min)));
            // Un seul texte de deux tailles : « EVE » et « de Constructo AI » sur la même ligne de base.
            let blanc = |font_id: FontId, espace: f32| TextFormat {
                font_id,
                color: Color32::WHITE,
                extra_letter_spacing: espace,
                ..Default::default()
            };
            let mut titre = LayoutJob::default();
            titre.append("EVE", 0.0, blanc(FontId::new(17.0, FontFamily::Name(GRAS.into())), 2.0));
            titre.append("de Constructo AI", 6.0, blanc(FontId::new(14.0, FontFamily::Name(SEMI.into())), 0.0));
            nom.label(titre);
            let bas = egui::Rect::from_min_size(egui::pos2(x, image.bottom() - SOUS_LA_LIGNE), vec2(420.0, 18.0));
            let mut qui_est_la =
                ui.new_child(egui::UiBuilder::new().max_rect(bas).layout(Layout::left_to_right(Align::Min)));
            qui_est_la.spacing_mut().item_spacing.x = 0.0;
            // Le profil actif ; avec plus d'un profil, son menu ([`profils`]).
            self.nom_et_profils(&mut qui_est_la);
            let qui = self.champ("qui");
            if !qui.is_empty() {
                qui_est_la.label(RichText::new(format!("  ·  {qui}")).size(12.0).color(couleurs::BARRE_TEXTE));
            }
            ui.allocate_rect(nom.min_rect().union(qui_est_la.min_rect()), Sense::hover());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                let r = bouton(ui, Some(icones::REGLAGES), "Réglages", Bouton::Barre(self.reglages_ouverts));
                self.bouton_reglages = r.rect;
                self.zones_de_la_barre.push(r.rect);
                if r.on_hover_text("La voix, l'écoute, ce poste ; quitter Eve").clicked() {
                    self.reglages_ouverts = !self.reglages_ouverts;
                    self.historique = None;
                }
                ui.add_space(6.0);
                let ouvert = self.historique.is_some();
                let r = bouton(ui, Some(historique_ui::icones::HISTORIQUE), "Historique", Bouton::Barre(ouvert));
                self.bouton_historique = r.rect;
                self.zones_de_la_barre.push(r.rect);
                if r.on_hover_text("Les conversations gardées sur ce poste").clicked() {
                    self.historique = if ouvert { None } else { Some(Historique::nouveau()) };
                    self.reglages_ouverts = false;
                }
                ui.add_space(6.0);
                if self.large {
                    // Le thème clair l'appelle comme l'ERP : le tableau de bord.
                    let (nom, aide) = match (self.theme_constructo(ui.ctx()), self.centre_ouvert) {
                        (true, true) => ("Tableau de bord", "Replier le tableau de bord"),
                        (true, false) => ("Tableau de bord", "Déplier le tableau de bord"),
                        (false, true) => ("Centre de commande", "Replier le centre de commande"),
                        (false, false) => ("Centre de commande", "Déplier le centre"),
                    };
                    let r = bouton(ui, Some(icones::VOLET), nom, Bouton::Barre(self.centre_ouvert));
                    self.zones_de_la_barre.push(r.rect);
                    if r.on_hover_text(aide).clicked() {
                        self.centre_ouvert = !self.centre_ouvert;
                    }
                    ui.add_space(4.0);
                    let (trait_, _) = ui.allocate_exact_size(vec2(1.0, 22.0), Sense::hover());
                    ui.painter().rect_filled(trait_, 0.0, Color32::from_white_alpha(28));
                    ui.add_space(4.0);
                }
                let oreille = match self.champ("oreille") {
                    "prete" => couleurs::PASTILLE_OK,
                    "erreur" => couleurs::CRIT,
                    _ => couleurs::AMBRE,
                };
                let micro = if self.etat.get("micro").is_some_and(|m| !m.is_null()) {
                    couleurs::PASTILLE_OK
                } else {
                    couleurs::CRIT
                };
                self.zones_de_la_barre.push(pastille(ui, "Oreille", oreille));
                self.zones_de_la_barre.push(pastille(ui, "Micro", micro));
                // Constructo AI, comme la page : branchée (vert), en panne (rouge), pas reliée (gris), en route (ambre).
                let constructo = &self.etat["constructo"];
                if constructo["permis"].as_bool().unwrap_or(false) {
                    let couleur = if constructo["branchee"].as_bool().unwrap_or(false) {
                        couleurs::PASTILLE_OK
                    } else if constructo["erreur"].as_str().is_some_and(|e| !e.is_empty()) {
                        couleurs::CRIT
                    } else if constructo["etat"].as_str() == Some("deconnectee") {
                        couleurs::PASTILLE_NEUTRE
                    } else {
                        couleurs::AMBRE
                    };
                    self.zones_de_la_barre.push(pastille(ui, "Constructo AI", couleur));
                }
                if !self.etat.get("propre").and_then(Value::as_bool).unwrap_or(false) {
                    let base = match self.champ("base") {
                        "connectee" => couleurs::PASTILLE_OK,
                        "absente" => couleurs::CRIT,
                        _ => couleurs::PASTILLE_NEUTRE,
                    };
                    self.zones_de_la_barre.push(pastille(ui, "Base", base));
                }
            });
        });
    }

    /// Le haut de la conversation : l'orbe, ce qu'Eve fait, comment lui parler ; la demande d'accord, s'il y en a une.
    fn tete(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            self.orbe(ui);
            ui.label(semi(self.champ("libelle"), 17.0).color(couleurs::ENCRE));
            let aide = self.champ("aide");
            if !aide.is_empty() {
                ui.label(RichText::new(aide).size(12.0).color(couleurs::ENCRE_3));
            }
            ui.add_space(12.0);
        });
        if let Some(a) = self.etat.get("accord").filter(|a| !a.is_null()) {
            self.accord(ui, a);
        }
    }

    /// L'orbe vivante de l'Eve de Constructo AI ([`crate::orbe`]) : 236 points comme dans la page (moins dans une
    /// fenêtre basse), 112 le temps d'une demande d'accord, pour lui laisser la place. Tenue, elle parle, comme la
    /// touche.
    fn orbe(&mut self, ui: &mut egui::Ui) {
        let cote = self.cote_de_l_orbe(ui.ctx());
        let (rect, rep) = ui.allocate_exact_size(vec2(cote, cote), Sense::click());
        self.orbe_dans(ui, rect, rep);
    }

    /// Le côté de l'orbe en tête de la conversation : 236 points au plus (moins dans une fenêtre basse), 112 le temps
    /// d'une demande d'accord ; le changement glisse.
    fn cote_de_l_orbe(&self, ctx: &egui::Context) -> f32 {
        let accord = self.etat.get("accord").is_some_and(|a| !a.is_null());
        let grande = (self.hauteur * 0.26).clamp(150.0, 236.0);
        ctx.animate_value_with_time(egui::Id::unique("cote-de-l-orbe"), if accord { 112.0 } else { grande }, 0.35)
    }

    /// L'orbe dessinée dans `rect`, qui répond par `rep` : tenue, elle parle, comme la touche. Le centre de commande la
    /// pose au cœur du réseau, le thème clair dans sa bannière.
    fn orbe_dans(&mut self, ui: &mut egui::Ui, rect: egui::Rect, rep: egui::Response) {
        self.orbe_tenue = rep.is_pointer_button_down_on();
        rep.on_hover_text("Tiens appuyé pour parler").on_hover_cursor(egui::CursorIcon::PointingHand);
        let etat = self.champ("etat").to_string();
        let niveau = self.etat.get("niveau").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let temps = ui.input(|i| i.time);
        self.orbe.dessiner(ui, rect, &etat, niveau, temps);
    }

    /// La demande d'accord : ce qui va partir, et les deux boutons, pour la demande affichée seulement (son numéro).
    fn accord(&self, ui: &mut egui::Ui, a: &Value) {
        let numero = a.get("numero").cloned().unwrap_or(Value::Null);
        let texte = |cle: &str| a.get(cle).and_then(Value::as_str).unwrap_or("").to_string();
        egui::Frame::new()
            .fill(couleurs::ALERTE_FOND)
            .stroke((1.0, couleurs::ALERTE_BORD))
            .corner_radius(12.0)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(icone(icones::ALERTE, 15.0).color(couleurs::AMBRE));
                    ui.label(semi("J'ai besoin de ton accord", 14.0).color(couleurs::ENCRE));
                });
                ui.label(RichText::new(texte("quoi")).size(13.5).color(couleurs::ENCRE));
                let detail = texte("detail");
                if !detail.is_empty() {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(couleurs::FOND)
                        .corner_radius(8.0)
                        .inner_margin(Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                                ui.label(RichText::new(detail).monospace().size(12.0).color(couleurs::ENCRE_2));
                            });
                        });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if bouton(ui, Some(icones::OUI), "Je confirme", Bouton::Plein(couleurs::OUI)).clicked() {
                        self.geste("confirmer", numero.clone());
                    }
                    if bouton(ui, Some(icones::NON), "Annuler", Bouton::Cerne).clicked() {
                        self.geste("annuler", numero.clone());
                    }
                    if let Some(n) = a.get("restants").and_then(Value::as_u64).filter(|n| *n > 0) {
                        ui.label(
                            RichText::new(format!("encore {n} après celle-ci")).size(11.5).color(couleurs::ENCRE_3),
                        );
                    }
                });
            });
        ui.add_space(10.0);
    }

    /// Le fil : les échanges, puis ce qu'Eve est en train d'écrire — ou trois points quand elle réfléchit. Défilé, il
    /// s'estompe sous ce qui le surmonte au lieu d'y être coupé net. `style` : en bulles, ou en flux (le tableau).
    fn fil(&mut self, ui: &mut egui::Ui, style: Style) {
        self.place_du_fil = ui.max_rect();
        let lignes = self.etat.get("fil").and_then(Value::as_array);
        let en_cours = self.etat.pointer("/en_cours/texte").and_then(Value::as_str).filter(|t| !t.trim().is_empty());
        let reflechit = self.etat.get("etat").and_then(Value::as_str) == Some("reflexion");
        // Le bouton du bas, cliqué à l'image d'avant : le fil y redescend (en douceur), et s'y tient de nouveau.
        let descendre = std::mem::take(&mut self.au_bas_du_fil);
        // Les cartes dessinées dans les bulles, et les gestes qu'un clic y demande (rendus au chef après le fil).
        let (cartes, mut gestes) = (&mut self.cartes, Vec::new());
        let zone = ui.max_rect();
        let sortie = egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            if lignes.is_none_or(|l| l.is_empty()) && en_cours.is_none() {
                ui.add_space(20.0);
                let discret = if style == Style::Claire { theme::erp::TEXTE_2 } else { couleurs::ENCRE_3 };
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("La conversation commence ici.").size(12.5).color(discret));
                });
            }
            for l in lignes.into_iter().flatten() {
                let texte = l.get("texte").and_then(Value::as_str).unwrap_or("");
                let de = l.get("de").and_then(Value::as_str).unwrap_or("");
                let heure = l.get("heure").and_then(Value::as_str).unwrap_or("");
                let ancien = l.get("ancien").and_then(Value::as_bool).unwrap_or(false);
                let pieces: Vec<&str> =
                    l.get("pieces").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                let tour = l.get("tour").and_then(Value::as_str).unwrap_or("");
                let ligne = Ligne { de, texte, heure, ancien, pieces: &pieces, en_cours: false, tour };
                match style {
                    Style::Bulles => bulle(ui, &ligne, cartes, &mut gestes),
                    Style::Flux => ligne_du_flux(ui, &ligne, cartes, &mut gestes),
                    Style::Claire => theme::bulle(ui, &ligne, cartes, &mut gestes),
                }
            }
            if let Some(t) = en_cours {
                let ligne =
                    Ligne { de: "eve", texte: t, heure: "", ancien: false, pieces: &[], en_cours: true, tour: "" };
                match style {
                    Style::Bulles => bulle(ui, &ligne, cartes, &mut gestes),
                    Style::Flux => ligne_du_flux(ui, &ligne, cartes, &mut gestes),
                    Style::Claire => theme::bulle(ui, &ligne, cartes, &mut gestes),
                }
            } else if reflechit && style == Style::Claire {
                theme::points_de_suspension(ui);
            } else if reflechit {
                points_de_suspension(ui);
            }
            ui.add_space(8.0);
            if descendre {
                ui.scroll_to_cursor(Some(Align::BOTTOM));
            }
        });
        // Remontée loin dans la conversation (plus de 160 points du bas), un bouton rond, au bas du fil, y ramène ; il
        // vient et s'en va en fondu.
        let reste = sortie.content_size.y - sortie.state.clamped_offset().y - sortie.inner_rect.height();
        let montre = ui.ctx().animate_bool_with_time(egui::Id::unique("aller-en-bas-se-montre"), reste > 160.0, 0.15);
        if montre > 0.0 {
            let rect =
                egui::Rect::from_center_size(egui::pos2(zone.center().x, zone.bottom() - 30.0), vec2(38.0, 38.0));
            let rep = ui.interact(rect, egui::Id::unique("aller-en-bas-du-fil"), Sense::click());
            bouton_du_bas(ui, rect, rep.hovered(), montre, style);
            let rep =
                rep.on_hover_text("Aller au bas de la conversation").on_hover_cursor(egui::CursorIcon::PointingHand);
            if rep.clicked() {
                self.au_bas_du_fil = true;
                ui.ctx().request_repaint();
            }
        }
        if sortie.state.clamped_offset().y > 1.0 {
            let fondu = egui::Rect::from_min_size(zone.left_top(), vec2(zone.width(), 22.0));
            match style {
                // Le fond clair respire : le fondu en prend la couleur, là où il est.
                Style::Claire => theme::fondu(ui.painter(), fondu),
                Style::Flux => degrade(ui.painter(), fondu, hud::FLUX, Color32::TRANSPARENT),
                Style::Bulles => degrade(ui.painter(), fondu, couleurs::SURFACE, Color32::TRANSPARENT),
            }
        }
        for g in gestes {
            // « Voir au tableau de bord », sous une carte épinglée : la fenêtre s'y rend elle-même.
            if g.action == "voir_epingle" {
                self.centre_ouvert = true;
                let id = g.valeur.as_str().unwrap_or("").to_string();
                self.epingle_a_montrer = Some((id, ui.input(|i| i.time), false));
                continue;
            }
            self.geste(g.action, g.valeur);
        }
    }

    /// La saisie : les pièces en attente ; le micro (à tenir pour parler), le trombone, le champ, l'envoi ; dessous,
    /// arrêter et recommencer.
    fn saisie(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        self.pieces_en_attente(ui, &pieces::SOMBRE, 0.0);
        let id = egui::Id::unique("saisie-texte");
        let actif = ui.ctx().memory(|m| m.has_focus(id));
        let bord = if actif { couleurs::ACCENT.gamma_multiply(0.7) } else { couleurs::FILET_FORT };
        egui::Frame::new()
            .fill(couleurs::FOND)
            .stroke((1.0, bord))
            .corner_radius(14.0)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(self.hauteur_de_la_saisie);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.bouton_micro(ui);
                    self.trombone(ui, &pieces::SOMBRE);
                    let largeur = (ui.available_width() - 42.0).max(60.0);
                    let allure = AllureDuChamp {
                        police: FontId::proportional(14.5),
                        encre: None,
                        indice: "Écris ta demande à Eve…".into(),
                        marge: Margin::symmetric(6, 9),
                        largeur,
                    };
                    let (champ, entree) = self.champ_de_saisie(ui, id, allure);
                    if std::mem::take(&mut self.saisie_au_clavier) {
                        champ.request_focus();
                    }
                    let pret = !self.saisie.trim().is_empty();
                    if (bouton_envoyer(ui, pret).clicked() || entree) && pret {
                        let t = std::mem::take(&mut self.saisie).trim().to_string();
                        self.geste("texte", Value::String(t));
                        champ.request_focus();
                    }
                });
            });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if bouton(ui, Some(icones::ARRETER), "Arrêter", Bouton::Plat).on_hover_text("Eve se tait").clicked() {
                self.geste("arreter", Value::Null);
            }
            let nouvelle = bouton(ui, Some(icones::NOUVELLE), "Nouvelle conversation", Bouton::Plat);
            if nouvelle.on_hover_text("Ctrl+N").clicked() {
                self.geste("nouvelle", Value::Null);
            }
        });
        ui.add_space(10.0);
    }

    /// Ce que montre la saisie (l'essai de la fenêtre le vérifie).
    pub(crate) fn texte_de_la_saisie(&self) -> &str {
        &self.saisie
    }

    /// Les demandes de la personne dans la conversation (les lignes « toi » du fil que publie le chef), de la première
    /// à la dernière.
    fn demandes_du_fil(&self) -> Vec<String> {
        let lignes = self.etat.get("fil").and_then(Value::as_array);
        lignes
            .map(|l| l.iter().filter(|x| x["de"] == "toi").filter_map(|x| x["texte"].as_str()).map(str::to_string))
            .into_iter()
            .flatten()
            .collect()
    }

    /// Le champ de la saisie, dans les deux thèmes : sur plusieurs lignes — Maj+Entrée va à la ligne, Entrée envoie —,
    /// il grandit avec le texte jusqu'à [`LIGNES_DE_LA_SAISIE`], puis défile. ↑ dans un champ vide, ou qui montre
    /// encore une demande reprise, reprend les demandes de la conversation, de la dernière à la première ; ↓ revient
    /// vers la plus récente, puis au champ vide. Rend sa réponse, et si Entrée vient de l'envoyer.
    fn champ_de_saisie(&mut self, ui: &mut egui::Ui, id: egui::Id, allure: AllureDuChamp) -> (egui::Response, bool) {
        let focus = ui.memory(|m| m.has_focus(id));
        if self.reprise.as_ref().is_some_and(|(_, t)| *t != self.saisie) {
            self.reprise = None;
        }
        // Les touches se prennent avant le champ : il les verrait, ↑ et ↓ pour bouger son curseur. Une touche avec
        // Maj ne compte pas (egui la prendrait pour la même) : Maj+Entrée va à la ligne, Maj+↑ choisit du texte.
        let prendre = |ui: &mut egui::Ui, touche: egui::Key| {
            ui.input_mut(|i| {
                let avant = i.events.len();
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                        if *key == touche && modifiers.is_none())
                });
                i.events.len() < avant
            })
        };
        if focus && (self.saisie.is_empty() || self.reprise.is_some()) {
            let haut = prendre(ui, egui::Key::ArrowUp);
            let bas = !haut && self.reprise.is_some() && prendre(ui, egui::Key::ArrowDown);
            if haut || bas {
                let demandes = self.demandes_du_fil();
                let avant = self.reprise.as_ref().map_or(0, |(k, _)| *k);
                let k = if haut { (avant + 1).min(demandes.len()) } else { avant.saturating_sub(1) };
                match k.checked_sub(1).and_then(|r| demandes.len().checked_sub(r + 1)).map(|i| &demandes[i]) {
                    Some(t) => {
                        self.saisie = t.clone();
                        self.reprise = Some((k, t.clone()));
                    }
                    None => {
                        self.saisie.clear();
                        self.reprise = None;
                    }
                }
                // Le curseur au bout de la demande reprise : on la complète, ou Entrée la renvoie.
                let mut etat = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                let fin = egui::text::CCursor::end_of_str(&self.saisie);
                etat.cursor.set_char_range(Some(egui::text::CCursorRange::one(fin)));
                etat.store(ui.ctx(), id);
            }
        }
        let envoyer = focus && prendre(ui, egui::Key::Enter);
        // Sa hauteur : celle de son texte, d'une ligne à six — donnée à la zone qui défile, qui en prendrait sinon 64 au
        // moins ; la rangée de la saisie la lit à l'image suivante pour y centrer ses boutons.
        let marges = f32::from(allure.marge.top) + f32::from(allure.marge.bottom);
        let interieur = (allure.largeur - f32::from(allure.marge.left) - f32::from(allure.marge.right)).max(20.0);
        let p = ui.painter();
        let ligne = p.layout_no_wrap("Ag".to_string(), allure.police.clone(), Color32::WHITE).size().y;
        let texte = p.layout(self.saisie.clone(), allure.police.clone(), Color32::WHITE, interieur).size().y;
        let hauteur = texte.clamp(ligne, ligne * LIGNES_DE_LA_SAISIE) + marges;
        self.hauteur_de_la_saisie = hauteur;
        // Sa marge dans son cadre : egui ne prend pas `margin` d'un champ qui a le sien.
        let mut champ = egui::TextEdit::multiline(&mut self.saisie)
            .id(id)
            .frame(egui::Frame::new().inner_margin(allure.marge))
            .font(allure.police)
            .hint_text(allure.indice)
            .desired_rows(1)
            .desired_width(allure.largeur)
            .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter));
        if let Some(c) = allure.encre {
            champ = champ.text_color(c);
        }
        // Sans la marge des zones qui défilent (celle de leurs barres, dans le thème) : le champ garde sa largeur, et
        // l'envoi sa place à côté de lui.
        let rep = ui.scope(|ui| {
            ui.spacing_mut().scroll.content_margin = Margin::ZERO;
            let defile = egui::ScrollArea::vertical()
                .id_salt(("defilement-de-la-saisie", id))
                .min_scrolled_height(hauteur)
                .max_height(hauteur);
            defile.show(ui, |ui| ui.add(champ)).inner
        });
        (rep.inner, envoyer)
    }

    /// Le micro, à tenir comme la touche (ou l'orbe, dessinée avant lui à chaque image) : la parole part quand il se
    /// relâche. Il passe au vert, et une onde s'en échappe, tant qu'Eve écoute.
    fn bouton_micro(&mut self, ui: &mut egui::Ui) {
        let (rect, rep) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
        self.tenir_pour_parler(rep.is_pointer_button_down_on() || self.orbe_tenue);
        let ecoute = self.parle || self.champ("etat") == "ecoute";
        let fond = match (ecoute, rep.hovered()) {
            (true, _) => couleurs::OUI,
            (false, true) => melange(couleurs::ACCENT, Color32::WHITE, 0.12),
            (false, false) => couleurs::ACCENT,
        };
        let (c, p) = (rect.center(), ui.painter());
        if ecoute {
            let f = (ui.input(|i| i.time) as f32 * 1.6).fract();
            p.circle_stroke(c, 18.0 + f * 5.0, (2.0, couleurs::OUI.gamma_multiply(1.0 - f)));
        }
        p.circle_filled(c, 18.0, fond);
        p.text(
            c,
            Align2::CENTER_CENTER,
            icones::MICRO,
            FontId::new(16.0, FontFamily::Name(ICONES.into())),
            couleurs::SUR_PLEIN,
        );
        rep.on_hover_text("Maintiens pour parler").on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    /// Le micro ou l'orbe sont tenus (`tenu`) : la parole commence ; relâchés, elle part.
    fn tenir_pour_parler(&mut self, tenu: bool) {
        if tenu && !self.parle {
            self.parle = true;
            self.geste("parler_debut", Value::Null);
        } else if !tenu && self.parle {
            self.parle = false;
            self.geste("parler_fin", Value::Null);
        }
    }

    /// La conversation, à droite du tableau, à la place de son flux des entrées et sorties : son titre, la demande
    /// d'accord, le fil, la saisie.
    fn conversation_du_flux(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("titre-du-flux").show_separator_line(false).frame(egui::Frame::new()).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(centre::capitales("CONVERSATION", 11.0, hud::CYAN).extra_letter_spacing(3.5));
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 11.0), Sense::hover());
            ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, hud::BORD));
            if let Some(a) = self.etat.get("accord").filter(|a| !a.is_null()) {
                self.accord(ui, a);
            }
        });
        egui::Panel::bottom("saisie")
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.saisie(ui));
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.fil(ui, Style::Flux));
    }

    /// Le centre de commande, au style du tableau HTML de l'Assistante famille : le réseau des actions sur toute la
    /// place, l'orbe vivante en son cœur (à la place du nœud « EVE »), et autour, comme dans la page, le titre et la
    /// ligne d'état en haut, les compteurs et l'horloge en haut à droite, les échéances à gauche, le panneau
    /// d'information en bas à gauche, la décision du jour en bas au centre, la courbe, la légende et les touches en
    /// bas. H : le réseau seul.
    fn centre_hud(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let temps = ui.input(|i| i.time);
        self.reseau.habiller(false, false);
        // Le thème « galaxie » : le même centre de commande, ses points en Voie lactée.
        let galaxie = self.theme(ui.ctx()) == "galaxie";
        self.reseau.allure(if galaxie { crate::reseau::Allure::Galaxie } else { crate::reseau::Allure::Cerveau });
        self.anime = self.reseau.dessiner(ui, rect, temps);
        // L'orbe au cœur du réseau ; tenue, elle parle ; réduite le temps d'un accord, comme dans la page.
        let accord = self.etat.get("accord").is_some_and(|a| !a.is_null());
        let grande = (rect.width().min(rect.height()) * 0.26).clamp(140.0, 300.0);
        let voulu = if accord { grande * 0.6 } else { grande };
        let cote = ui.ctx().animate_value_with_time(egui::Id::unique("cote-de-l-orbe-du-centre"), voulu, 0.35);
        let r_orbe = egui::Rect::from_center_size(Reseau::centre_de(rect), vec2(cote, cote));
        let rep = ui.interact(r_orbe, egui::Id::unique("orbe-du-centre"), Sense::click());
        self.orbe_dans(ui, r_orbe, rep);
        let aujourdhui = journal::date();
        let horodatage = journal::horodatage();
        let heure = horodatage.get(11..16).unwrap_or("").replace('-', ":");
        // L'ambiance de la page, même en mode propre : la saison, la fête, le balayage, la nuit (sur le réseau et
        // l'orbe, pas sur les panneaux) ; la phrase de la fête, tapée par le réseau.
        let fete = ambiance::fete_du_jour(&self.centre.famille(), &aujourdhui);
        let moment = Moment {
            date: &aujourdhui,
            heure: horodatage.get(11..13).and_then(|h| h.parse().ok()).unwrap_or(12),
            econome: self.reseau.modes().econome,
            il_neige: self.bulletin.as_ref().is_some_and(|b| b.il_neige),
            fete: fete.as_ref(),
        };
        self.anime |= self.ambiance.dessiner(ui.painter(), rect, temps, &moment);
        if let Some(souhait) = self.ambiance.souhait(temps) {
            self.reseau.annoncer(souhait, temps);
        }
        if !self.propre {
            self.panneaux_hud(ui, rect, temps, &aujourdhui, &horodatage, &heure);
        }
        // L'écran de veille, par-dessus tout : il attend tant qu'Eve écoute, travaille ou parle.
        let modes = self.reseau.modes();
        let bloquee = self.reseau.compteurs().1 > 0
            || modes.demo
            || modes.rediffusion
            || matches!(self.champ("etat"), "ecoute" | "transcription" | "reflexion" | "parole" | "confirmation");
        let echeance = prochaine_echeance(&self.centre.echeances_lues(), &aujourdhui);
        let (heure_de_veille, date_longue) =
            (ambiance::heure_de_veille(&horodatage), ambiance::date_longue(&aujourdhui));
        let contenu = Veille {
            heure: &heure_de_veille,
            date: &date_longue,
            meteo: self.bulletin.as_ref().map_or("", |b| b.ligne.as_str()),
            echeance: &echeance,
        };
        self.ambiance.veille(ui.painter(), rect, temps, bloquee, &contenu);
    }

    /// Les panneaux autour du réseau, que le mode propre retire : le haut, les échéances, le panneau d'information, la
    /// décision du jour, le bas.
    fn panneaux_hud(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        temps: f64,
        aujourdhui: &str,
        horodatage: &str,
        heure: &str,
    ) {
        self.hud_haut(ui, rect, temps, horodatage);
        // Les échéances, en haut à gauche.
        let gauche = egui::Rect::from_min_size(rect.left_top() + vec2(20.0, 92.0), vec2(290.0, rect.height() * 0.45));
        ui.scope_builder(egui::UiBuilder::new().max_rect(gauche), |ui| {
            self.centre.echeances_hud(ui, aujourdhui, temps);
        });
        // Le panneau d'information, en bas à gauche, au-dessus de la courbe, et la décision du jour, en bas au centre :
        // posés par leur bas, à la hauteur qu'ils avaient à l'image d'avant (elle se rattrape en une image).
        let bas = rect.bottom() - 98.0;
        let (id_panneau, id_decision) =
            (egui::Id::unique("hauteur-du-panneau"), egui::Id::unique("hauteur-de-la-decision"));
        let (h_panneau, h_decision) = ui
            .ctx()
            .data(|d| (d.get_temp::<f32>(id_panneau).unwrap_or(240.0), d.get_temp::<f32>(id_decision).unwrap_or(90.0)));
        let panneau = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 20.0, bas - h_panneau),
            egui::pos2(rect.left() + 330.0, bas),
        );
        let h1 = ui
            .scope_builder(egui::UiBuilder::new().max_rect(panneau), |ui| {
                self.centre.panneau_hud(ui, aujourdhui, heure, temps);
            })
            .response
            .rect
            .height();
        // La décision du jour, à 128 px du bas comme la page : centrée entre le panneau de gauche et la conversation, et
        // pas plus large qu'une phrase qui se lit d'un coup d'œil.
        let (de, a) = (rect.left() + 300.0, rect.right() - 14.0);
        let (milieu, large) = ((de + a) / 2.0, (a - de).min(720.0));
        let decision = egui::Rect::from_min_max(
            egui::pos2(milieu - large / 2.0, rect.bottom() - 128.0 - h_decision),
            egui::pos2(milieu + large / 2.0, rect.bottom() - 128.0),
        );
        let h2 = ui
            .scope_builder(egui::UiBuilder::new().max_rect(decision).layout(Layout::top_down(Align::Center)), |ui| {
                self.centre.decision_hud(ui, aujourdhui, temps);
            })
            .response
            .rect
            .height();
        if (h1 - h_panneau).abs() > 0.5 || (h2 - h_decision).abs() > 0.5 {
            ui.ctx().data_mut(|d| {
                d.insert_temp(id_panneau, h1);
                d.insert_temp(id_decision, h2);
            });
            ui.ctx().request_repaint();
        }
        self.hud_bas(ui, rect, temps);
    }

    /// Les réglages, comme la page (eve.html : 381-446, 639-684) : un panneau par-dessus la fenêtre, sous son bouton de
    /// la barre — la voix et la sourdine, qui est devant le poste, l'écoute, les alertes vocales (l'Adjointe seulement),
    /// ce poste ; le raccourci ; « Quitter Eve ». ×, Échap, un clic dehors ou une demande d'accord le ferment.
    fn reglages(&mut self, ctx: &egui::Context) {
        let maintenant = ctx.input(|i| i.time);
        self.tenus.retain(|_, (_, t)| maintenant - *t < TENUE);
        let accord = self.etat.get("accord").is_some_and(|a| !a.is_null());
        if accord || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.reglages_ouverts = false;
        }
        let ouverture =
            ctx.animate_bool_with_time(egui::Id::unique("ouverture-des-reglages"), self.reglages_ouverts, 0.18);
        if ouverture <= 0.0 {
            return;
        }
        let ecran = ctx.content_rect();
        let large = (ecran.width() - 28.0).min(380.0);
        // Il entre en glissant de 8 points, comme la page (`panneau-entre`).
        let haut = self.bouton_reglages.bottom() + 8.0 + 8.0 * (1.0 - ouverture);
        let hauteur_max = (ecran.bottom() - haut - 14.0).max(160.0);
        let clair = self.theme_constructo(ctx);
        let zone = egui::Area::new(egui::Id::unique("panneau-des-reglages"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(ecran.right() - 14.0 - large, haut))
            .show(ctx, |ui| {
                if clair {
                    theme::visuels_du_panneau(ui.visuals_mut());
                }
                let p = panneau(ui);
                ui.multiply_opacity(ouverture);
                egui::Frame::new().fill(p.fond).stroke((1.0, p.filet)).corner_radius(14).shadow(p.ombres[1]).show(
                    ui,
                    |ui| {
                        ui.set_width(large);
                        self.tete_des_reglages(ui);
                        egui::ScrollArea::vertical().max_height(hauteur_max - 52.0).show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin { left: 14, right: 14, top: 0, bottom: 14 })
                                .show(ui, |ui| self.corps_des_reglages(ui, maintenant));
                        });
                    },
                );
            });
        // Un clic dehors les ferme ; pas celui du bouton de la barre, qui les a déjà basculés, ni un clic dans une liste
        // qu'ils ont ouverte (celle des voix déborde du panneau, sur sa propre couche, au-dessus).
        let dedans = zone.response.rect;
        let dehors =
            ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()).is_some_and(|p| {
                let au_dessus = ctx.layer_id_at(p).is_some_and(|c| c.order >= egui::Order::Foreground);
                !dedans.contains(p) && !au_dessus && !self.bouton_reglages.contains(p)
            });
        if dehors {
            self.reglages_ouverts = false;
        }
    }

    /// L'historique des conversations (le lot 6.3 ; la page : 686-709) : un panneau par-dessus la conversation, à la
    /// place du fil, que le bouton de la barre ouvre et ferme. Échap revient à la liste, puis ferme ; un clic dehors (il
    /// va aussi à ce qu'il touche, comme dans la page) ou une demande d'accord le ferment.
    fn historique(&mut self, ctx: &egui::Context) {
        if self.historique.is_none() {
            return;
        }
        let accord = self.etat.get("accord").is_some_and(|a| !a.is_null());
        let echap = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let qui = self.champ("qui").to_string();
        let place = self.place_du_fil.expand2(vec2(6.0, 0.0));
        let clair = self.theme_constructo(ctx);
        let Some(h) = self.historique.as_mut() else { return };
        if accord || (echap && h.echap()) {
            self.historique = None;
            return;
        }
        let cartes = &mut self.cartes;
        let sortie = egui::Area::new(egui::Id::unique("panneau-historique"))
            .order(egui::Order::Foreground)
            .fixed_pos(place.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                if clair {
                    theme::visuels_du_panneau(ui.visuals_mut());
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(place), |ui| h.dessiner(ui, &qui, cartes)).inner
            })
            .inner;
        match sortie {
            Some(Sortie::Fermer) => self.historique = None,
            Some(Sortie::Reprendre(id)) => {
                self.geste("reprendre", Value::String(id));
                self.historique = None;
            }
            Some(Sortie::Geste(g)) => self.geste(g.action, g.valeur),
            None => {}
        }
        let dehors = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.press_origin().is_some_and(|p| !place.contains(p) && !self.bouton_historique.contains(p))
        });
        if dehors {
            self.historique = None;
        }
    }

    /// La tête du panneau des réglages : le ruban, « Réglages », le bouton qui ferme.
    fn tete_des_reglages(&mut self, ui: &mut egui::Ui) {
        let p = panneau(ui);
        egui::Frame::new().inner_margin(Margin { left: 14, right: 9, top: 9, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (ruban, _) = ui.allocate_exact_size(vec2(5.0, 20.0), Sense::hover());
                ui.painter().rect_filled(ruban, 2.5, p.accent);
                ui.add_space(4.0);
                let titre = RichText::new("Réglages").family(FontFamily::Name(GRAS.into())).size(16.0);
                ui.label(titre.color(p.encre));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if bouton_fermer(ui).on_hover_text("Fermer (Échap)").clicked() {
                        self.reglages_ouverts = false;
                    }
                });
            });
        });
        let (trait_, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(trait_, 0.0, p.filet);
    }

    /// Le corps du panneau des réglages, dans l'ordre de la page.
    fn corps_des_reglages(&mut self, ui: &mut egui::Ui, maintenant: f64) {
        let p = panneau(ui);
        ui.spacing_mut().item_spacing.y = 0.0;
        // La voix : la liste des voix du poste et la sourdine.
        rubrique(ui, "Voix");
        let liste: Vec<(String, String)> = self.etat["voix"]["liste"]
            .as_array()
            .map(|l| {
                l.iter().filter_map(|v| Some((v["nom"].as_str()?.to_string(), v["id"].as_str()?.to_string()))).collect()
            })
            .unwrap_or_default();
        let choisie = self.etat["voix"]["choisie"].as_str().unwrap_or("").to_string();
        let affichee = match liste.iter().find(|(_, id)| *id == choisie) {
            Some((nom, _)) => nom.clone(),
            None if liste.is_empty() => "Aucune voix pour l'instant".to_string(),
            None => "Choisis une voix".to_string(),
        };
        ui.add_enabled_ui(!liste.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("voix-d-eve")
                .width(ui.available_width())
                .selected_text(affichee)
                .popup_style(style_de_la_liste(ui))
                .show_ui(ui, |ui| {
                    for (nom, id) in &liste {
                        if ui.selectable_label(*id == choisie, nom).clicked() && *id != choisie {
                            self.geste("voix", Value::String(id.clone()));
                        }
                    }
                });
        });
        ui.add_space(8.0);
        let aide = "Eve ne parle plus à voix haute : elle répond par écrit et vous écoute toujours";
        groupe(ui, |ui| {
            self.interrupteur_de(ui, "Sourdine : Eve répond par écrit", "sourdine", true, Some(aide), maintenant)
        });
        // Qui est devant le poste : le champ se retient à Entrée ou quand on le quitte.
        rubrique(ui, "Qui est devant le poste");
        let id = egui::Id::unique("qui-est-devant-le-poste");
        let publie = self.champ("qui").to_string();
        if publie != self.qui_vu && !ui.memory(|m| m.has_focus(id)) {
            self.qui_saisi = publie.clone();
            self.qui_vu = publie.clone();
        }
        let champ = egui::TextEdit::singleline(&mut self.qui_saisi)
            .id(id)
            .hint_text("Votre prénom")
            .char_limit(40)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(10, 8));
        let rep = ui.add(champ).on_hover_text("Entrée pour le retenir : Eve ouvre une nouvelle conversation");
        if rep.lost_focus() {
            let q = self.qui_saisi.trim().to_string();
            if !q.is_empty() && q != publie {
                self.geste("qui", Value::String(q));
            }
        }
        // Les profils de cette Eve, l'actif coché ([`profils`]) ; son cerveau, le modèle et l'effort de Claude.
        self.rubrique_des_profils(ui);
        self.choix_du_cerveau(ui);
        // Constructo AI : la connexion de cette Eve à l'ERP de l'entreprise (le privilège « constructo »), comme la page.
        let constructo = self.etat.get("constructo").cloned().unwrap_or(Value::Null);
        if constructo["permis"].as_bool().unwrap_or(false) {
            rubrique(ui, "Constructo AI");
            let etat = constructo["etat"].as_str().unwrap_or("");
            let connectee = constructo["connectee"].as_bool().unwrap_or(false);
            let erreur = constructo["erreur"].as_str().unwrap_or("");
            groupe(ui, |ui| {
                ui.add_space(9.0);
                let detail = constructo["detail"].as_str().unwrap_or("");
                ui.label(RichText::new(detail).size(13.5).color(p.encre_2));
                if !erreur.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(erreur).size(13.5).color(p.crit));
                }
                ui.add_space(9.0);
                ui.horizontal_wrapped(|ui| {
                    if !connectee || !erreur.is_empty() {
                        let texte = match (etat, connectee) {
                            ("en_cours", _) => "Rouvrir la page de connexion",
                            (_, true) => "Réessayer",
                            _ => "Se connecter à Constructo AI",
                        };
                        if bouton(ui, None, texte, Bouton::Plein(p.accent)).clicked() {
                            self.geste("constructo_connecter", Value::Null);
                        }
                    }
                    if (connectee || etat == "en_cours") && bouton(ui, None, "Se déconnecter", Bouton::Cerne).clicked()
                    {
                        self.geste("constructo_deconnecter", Value::Null);
                    }
                });
                ui.add_space(10.0);
            });
        }
        // L'écoute.
        rubrique(ui, "Écoute");
        groupe(ui, |ui| {
            let aide = "Eve écoute en continu et répond quand une phrase commence par « Eve »";
            self.interrupteur_de(ui, "Mot d'éveil : Eve", "eveil", true, Some(aide), maintenant);
            self.interrupteur_de(
                ui,
                "M'interrompre de la voix (« Eve, stop »)",
                "interruption",
                false,
                None,
                maintenant,
            );
        });
        // Les alertes vocales : celles de l'Adjointe d'Entrepreneur Général ; un spécialiste n'en a pas.
        if !self.etat.get("propre").and_then(Value::as_bool).unwrap_or(false) {
            rubrique(ui, "Alertes vocales");
            groupe(ui, |ui| {
                self.interrupteur_de(ui, "Courriels suspects", "alertes.suspects", true, None, maintenant);
                self.interrupteur_de(ui, "Factures reçues", "alertes.factures", false, None, maintenant);
                self.interrupteur_de(ui, "Autres pièces et numérisations", "alertes.pieces", false, None, maintenant);
                self.interrupteur_de(
                    ui,
                    "Rendez-vous, 15 minutes avant",
                    "alertes.rendez_vous",
                    false,
                    None,
                    maintenant,
                );
            });
        }
        // Le thème de la fenêtre : le centre de commande, ou celui de Constructo AI ([`theme`]).
        rubrique(ui, "Thème");
        self.choix_du_theme(ui);
        self.choix_du_portrait(ui);
        // Ce poste.
        rubrique(ui, "Sur ce poste");
        groupe(ui, |ui| {
            self.interrupteur_de(ui, "Toujours devant les autres fenêtres", "devant", true, None, maintenant);
            self.interrupteur_de(ui, "Démarrer avec Windows", "demarrage", false, None, maintenant);
            let quand =
                "Quand la fenêtre n'est pas devant : un rappel, une réponse longue finie, un accord qu'Eve attend";
            self.interrupteur_de(ui, "Notifications de Windows", "notifications", false, Some(quand), maintenant);
            let aide = "Sur ce poste seulement, par personne";
            self.interrupteur_de(
                ui,
                "Garder l'historique des conversations",
                "historique",
                false,
                Some(aide),
                maintenant,
            );
        });
        // La version en place, et le retour à la précédente (mise_a_jour.rs) : Eve y redémarre à la fin de sa réponse,
        // et la conversation reprend.
        let version = self.etat.get("version").cloned().unwrap_or(Value::Null);
        if let Some(texte) = version["texte"].as_str() {
            rubrique(ui, "Version");
            groupe(ui, |ui| {
                ui.add_space(9.0);
                ui.label(RichText::new(format!("Eve {texte}")).size(13.5).color(p.encre_2));
                if let Some(precedente) = version["precedente"].as_str() {
                    ui.add_space(9.0);
                    if version["relance"].as_bool().unwrap_or(false) {
                        let attente = "Eve redémarre à la fin de sa réponse.";
                        ui.label(RichText::new(attente).size(13.5).color(p.encre_2));
                    } else if bouton(
                        ui,
                        None,
                        &format!("Revenir à la version précédente ({precedente})"),
                        Bouton::Cerne,
                    )
                    .on_hover_text(
                        "Eve redémarre dans sa version d'avant, à la fin de sa réponse ; la conversation reprend",
                    )
                    .clicked()
                    {
                        self.geste("revenir", Value::Null);
                    }
                }
                ui.add_space(10.0);
            });
        }
        // Le raccourci, ses touches dessinées comme des touches.
        if let Some(raccourci) = self.etat["reglages"]["raccourci"].as_str().filter(|r| !r.is_empty()) {
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (i, t) in raccourci.split('+').enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("+").size(12.5).color(p.encre_2));
                    }
                    touche_de_clavier(ui, t);
                }
                ui.label(RichText::new(" affiche ou cache Eve, de n'importe où").size(12.5).color(p.encre_2));
            });
        }
        ui.add_space(10.0);
        if bouton_quitter(ui).clicked() {
            self.geste("quitter", Value::Null);
        }
    }

    /// La rubrique « Cerveau » des réglages : le modèle de Claude et son effort, en segments, parmi ceux qu'Eve offre
    /// (`reglages::MODELES`, `reglages::EFFORTS`) ; les gestes `modele` et `effort` au chef, qui les inscrit dans
    /// reglages.json et relance Claude Code avec eux entre deux demandes, la conversation reprise. Un choix fait d'un
    /// clic est tenu le temps que le chef le publie ([`TENUE`]).
    fn choix_du_cerveau(&mut self, ui: &mut egui::Ui) {
        rubrique(ui, "Cerveau");
        let modeles: Vec<(&str, String)> = crate::reglages::MODELES.iter().map(|m| (m.0, m.1.to_string())).collect();
        let efforts: Vec<(&str, String)> =
            crate::reglages::EFFORTS.iter().map(|e| (e.0, format!("Effort {}", e.1))).collect();
        for (cle, choix) in [("modele", modeles), ("effort", efforts)] {
            let tenu = egui::Id::unique(("cerveau-choisi-d-un-clic", cle));
            let maintenant = ui.input(|i| i.time);
            let actuel = match ui.ctx().data(|d| d.get_temp::<(String, f64)>(tenu)) {
                Some((v, quand)) if maintenant - quand < TENUE => v,
                _ => self.etat["reglages"][cle].as_str().unwrap_or("").to_string(),
            };
            let choisi = choix.iter().position(|(id, _)| *id == actuel).unwrap_or(usize::MAX);
            let noms: Vec<&str> = choix.iter().map(|(_, nom)| nom.as_str()).collect();
            if let Some(k) = segments(ui, cle, &noms, choisi)
                && k != choisi
            {
                let id = choix[k].0;
                ui.ctx().data_mut(|d| d.insert_temp(tenu, (id.to_string(), maintenant)));
                self.geste(cle, Value::String(id.into()));
            }
            ui.add_space(6.0);
        }
        let aide = "Opus et l'effort max réfléchissent plus loin, mais répondent plus lentement et coûtent plus cher. Eve \
                    change entre deux demandes ; la conversation continue, et ses agents prennent le même cerveau.";
        ui.label(RichText::new(aide).size(12.0).color(panneau(ui).encre_3));
    }

    /// Une rangée d'interrupteur des réglages : sa valeur (tenue quatre secondes après un clic), et le geste `reglage`
    /// quand on la change.
    fn interrupteur_de(
        &mut self,
        ui: &mut egui::Ui,
        texte: &str,
        cle: &str,
        premier: bool,
        aide: Option<&str>,
        maintenant: f64,
    ) {
        let valeur = valeur_reglage(&self.etat, &self.tenus, cle, maintenant);
        let mut rep = rangee_interrupteur(ui, texte, valeur, premier, egui::Id::unique(("interrupteur", cle)));
        if let Some(a) = aide {
            rep = rep.on_hover_text(a);
        }
        if rep.clicked()
            && let Some(v) = valeur
        {
            self.geste("reglage", serde_json::json!({ "cle": cle, "valeur": !v }));
            self.tenus.insert(cle.to_string(), (!v, maintenant));
        }
    }

    /// Le haut du tableau : son titre à gauche, la ligne d'état au milieu (l'action qui court, sinon ce que fait Eve,
    /// et comment lui parler), les compteurs et l'horloge à droite.
    fn hud_haut(&self, ui: &egui::Ui, rect: egui::Rect, temps: f64, horodatage: &str) {
        let p = ui.painter();
        // Le titre, au style de celui de la page (cdc:13-14) ; la barre d'Eve porte déjà son nom : le centre dit ce
        // qu'il montre, en face de « CONVERSATION ».
        let coin = rect.left_top() + vec2(20.0, 14.0);
        let titre = texte_hud(p, coin, Align2::LEFT_TOP, self.reseau.titre(), 15.0, hud::CYAN, 4.8);
        let sous_titre = "ACTIVITÉ EN DIRECT · 360°";
        texte_hud(p, egui::pos2(coin.x, titre.bottom() + 3.0), Align2::LEFT_TOP, sous_titre, 11.0, hud::GRIS, 1.3);
        // La ligne d'état (cdc:19-20), tapée par le réseau ; sous elle, comment parler à Eve.
        let haut = egui::pos2(rect.center().x, rect.top() + 62.0);
        let ligne = ligne_d_etat_hud(p, haut, &self.reseau.ligne_d_etat(temps));
        let aide = self.champ("aide");
        if !aide.is_empty() {
            let sous = egui::pos2(haut.x, ligne.bottom() + 8.0);
            texte_hud(p, sous, Align2::CENTER_TOP, &aide.to_uppercase(), 10.0, hud::GRIS, 1.2);
        }
        // Les compteurs (cdc:15-18) : le total des actions, celles en cours ; puis l'heure et la date.
        let (total, en_cours) = self.reseau.compteurs();
        let x = rect.right() - 20.0;
        let mut y = rect.top() + 14.0;
        let pluriel = if total > 1 { "ACTIONS" } else { "ACTION" };
        for (nombre, mot, couleur) in
            [(total.to_string(), pluriel, hud::CYAN), (en_cours.to_string(), "EN COURS", hud::VERT)]
        {
            y = texte_hud(p, egui::pos2(x, y), Align2::RIGHT_TOP, &nombre, 22.0, couleur, 0.0).bottom();
            y = texte_hud(p, egui::pos2(x, y), Align2::RIGHT_TOP, mot, 11.0, hud::GRIS, 1.1).bottom() + 2.0;
        }
        let h = texte_hud(p, egui::pos2(x, y + 4.0), Align2::RIGHT_TOP, &horloge(horodatage), 12.0, hud::HORLOGE, 0.0);
        if let Some(b) = &self.bulletin {
            texte_hud(p, egui::pos2(x, h.bottom() + 2.0), Align2::RIGHT_TOP, &b.ligne, 12.0, hud::AMBRE, 1.0);
        }
    }

    /// Le bas du tableau : la courbe d'activité (cdc:433-437), la légende et l'état de la liaison (cdc:88-93), l'aide
    /// des touches (cdc:95).
    fn hud_bas(&self, ui: &egui::Ui, rect: egui::Rect, temps: f64) {
        let p = ui.painter();
        let gauche = rect.left() + 20.0;
        // La courbe : les 90 dernières cases de 0,7 s, rapportées à la plus haute (trois au moins), en un trait
        // lumineux dans son cadre ; son halo, la lueur de la page.
        let boite = egui::Rect::from_min_max(
            egui::pos2(gauche, rect.bottom() - 86.0),
            egui::pos2(gauche + 360.0, rect.bottom() - 52.0),
        );
        p.rect_stroke(boite, 0.0, Stroke::new(1.0, hud::CYAN.gamma_multiply(0.25)), egui::StrokeKind::Middle);
        let cases = self.reseau.activite(temps);
        if cases.len() > 1 {
            let pas = boite.width() / (cases.len() - 1) as f32;
            let points: Vec<egui::Pos2> = cases
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let y = boite.bottom() - v.clamp(0.0, 1.0) * (boite.height() - 4.0);
                    egui::pos2(boite.left() + i as f32 * pas, y)
                })
                .collect();
            p.add(egui::Shape::line(points.clone(), Stroke::new(5.0, hud::CYAN.gamma_multiply(0.14))));
            p.add(egui::Shape::line(points, Stroke::new(1.8, hud::CYAN)));
        }
        let titre = "ACTIVITÉ · 60 DERNIÈRES SECONDES";
        texte_hud(p, egui::pos2(gauche, boite.top() - 3.0), Align2::LEFT_BOTTOM, titre, 10.0, hud::SOMBRE, 0.0);
        // La légende, puis l'état de la liaison, ou le mode du réseau, comme la page le dit à cette place.
        let y = rect.bottom() - 21.0;
        let mut x = gauche;
        for (mot, c) in [("ENTRÉE (requête d'Eve)", hud::CYAN), ("SORTIE (réponse reçue)", hud::VERT)] {
            p.circle_filled(egui::pos2(x + 4.0, y), 7.0, c.gamma_multiply(0.12));
            p.circle_filled(egui::pos2(x + 4.0, y), 4.0, c);
            x = texte_hud(p, egui::pos2(x + 14.0, y), Align2::LEFT_CENTER, mot, 11.0, hud::SOMBRE, 0.9).right() + 18.0;
        }
        if !self.personnes_vues.is_empty() {
            for personne in &self.personnes_vues {
                let [r, v, b] = personne.couleur;
                p.circle_filled(egui::pos2(x + 4.0, y), 4.0, Color32::from_rgb(r, v, b));
                let nom = personne.nom.trim().to_uppercase();
                x = texte_hud(p, egui::pos2(x + 14.0, y), Align2::LEFT_CENTER, &nom, 11.0, hud::SOMBRE, 0.9).right()
                    + 7.0;
            }
            x += 11.0;
        }
        texte_hud(
            p,
            egui::pos2(x, y),
            Align2::LEFT_CENTER,
            etat_de_la_liaison(self.reseau.modes()),
            11.0,
            hud::SOMBRE,
            0.9,
        );
        // L'aide des touches, entre le panneau de gauche et la conversation, à la ligne quand la place manque.
        let son = match (self.musique.is_some(), self.sons_allumes) {
            (false, _) => "SON : AUCUNE SORTIE AUDIO",
            (true, true) => "SON ACTIF · S POUR COUPER",
            (true, false) => "SON COUPÉ · S",
        };
        let touches = format!(
            "GLISSER : TOURNER À 360° · MOLETTE : ZOOM · R : ROTATION AUTO · D : DÉMO · H : MODE PROPRE · \
             E : MODE ÉCONOME · J : REJOUER LA JOURNÉE · A : MUSIQUE · F : PLEIN ÉCRAN · {son}"
        );
        let (debut, fin) = (rect.left() + 400.0, rect.right() - 14.0);
        if fin - debut > 160.0 {
            let mut job = LayoutJob::default();
            job.wrap.max_width = fin - debut;
            let format = TextFormat {
                font_id: FontId::monospace(11.0),
                color: hud::SOMBRE,
                extra_letter_spacing: 0.9,
                ..Default::default()
            };
            job.append(&touches, 0.0, format);
            let galley = p.layout_job(job);
            p.galley(egui::pos2(debut, rect.bottom() - 58.0 - galley.size().y), galley, hud::SOMBRE);
        }
    }
}

impl eframe::App for Fenetre {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // La cadence : la carte graphique finit l'image d'avant avant que la suivante se prépare, une fois par image
        // (sa première passe). Sans cela, le pilote en préparait d'avance et le bureau, qui compose l'écran à chaque
        // trame, en montrait deux à la même trame : une image sur cinq se voyait deux fois et sa voisine jamais (mesuré
        // le 2026-10-25 dans la vraie fenêtre, 21 % des compositions) — le visage d'Eve avançait par à-coups. Ainsi,
        // chaque image se voit une fois, deux trames après son calcul, à la milliseconde près ([`retard_de_l_ecran`]).
        if ui.ctx().current_pass_index() == 0
            && let Some(gl) = frame.gl()
        {
            use eframe::glow::HasContext;
            // SAFETY : le contexte d'OpenGL de la fenêtre est le contexte courant pendant `ui` (eframe l'y met).
            unsafe { gl.finish() };
        }
        let ctx = ui.ctx().clone();
        if let Some(s) = self.dernier.lock().ok().and_then(|mut x| x.take())
            && let Ok(v) = serde_json::from_str::<Value>(&s)
        {
            self.etat = v;
            self.suivre_les_outils(ctx.input(|i| i.time));
            self.suivre_les_dossiers_des_profils();
            // Une phrase mal entendue (le garde-fou de l'oreille) : dans la saisie, à corriger, une fois chacune.
            if let Some(n) = self.etat["a_corriger"]["numero"].as_u64()
                && n > self.corrigee
            {
                self.corrigee = n;
                self.saisie = self.etat["a_corriger"]["texte"].as_str().unwrap_or("").to_string();
                self.saisie_au_clavier = true;
            }
        }
        // La note qu'on écrit au tableau de bord se dit au chef une demi-seconde après sa dernière frappe ; en quittant,
        // tout de suite.
        let maintenant = ctx.input(|i| i.time);
        self.suivre_la_note(maintenant);
        // Devant ou non (au premier plan, pas réduite) : dit au chef à chaque changement, pour ses notifications.
        let devant = ctx.input(|i| i.viewport().focused.unwrap_or(true) && !i.viewport().minimized.unwrap_or(false));
        if self.devant_dit != Some(devant) {
            self.devant_dit = Some(devant);
            self.geste("premier_plan", Value::Bool(devant));
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.fermee {
            self.fermee = true;
            self.fermer_la_note(maintenant);
            self.geste("quitter", Value::Null);
        }
        // La barre d'espace tenue hors de la saisie parle, comme la touche (l'aide le dit) ; pas les réglages ni
        // l'historique ouverts, comme la page, ni un menu ouvert (celui des profils).
        let libre = ctx.memory(|m| m.focused().is_none())
            && !self.reglages_ouverts
            && self.historique.is_none()
            && !egui::Popup::is_any_open(&ctx);
        let espace = ctx.input(|i| i.key_down(egui::Key::Space)) && libre;
        if espace != self.espace {
            self.espace = espace;
            self.geste(if espace { "parler_debut" } else { "parler_fin" }, Value::Null);
        }
        // Ctrl+N : une nouvelle conversation, comme son bouton sous la saisie — de partout, la saisie comprise.
        let nouvelle = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
        if ctx.input_mut(|i| i.consume_shortcut(&nouvelle)) {
            self.geste("nouvelle", Value::Null);
        }
        // Les touches du tableau, hors de la saisie et des réglages : H le mode propre (le réseau seul), F le plein
        // écran, A la musique, S les sons, D la démonstration, J la journée rejouée, E le mode économe, R la rotation
        // automatique. Le thème « Constructo AI » n'a pas de tableau : pas de touches.
        let constructo = self.theme_constructo(&ctx);
        if libre && !constructo {
            let touche = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
            if touche(egui::Key::H) {
                self.propre = !self.propre;
            }
            if touche(egui::Key::F) {
                let plein = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!plein));
            }
            if touche(egui::Key::A) {
                self.musique_allumee = !self.musique_allumee;
            }
            if touche(egui::Key::S) {
                self.sons_allumes = !self.sons_allumes;
            }
            let modes = self.reseau.modes();
            if touche(egui::Key::D) {
                self.reseau.demo(!modes.demo);
            }
            if touche(egui::Key::E) {
                self.reseau.econome(!modes.econome);
            }
            if touche(egui::Key::R) {
                self.reseau.rotation(modes.immobile);
            }
            if touche(egui::Key::J) {
                // Les outils du jour, lus dans le journal d'Eve (et le précédent, mis de côté au-delà de 5 Mo) ; une
                // seconde fois, la rediffusion s'arrête.
                let evenements = if modes.rediffusion {
                    Vec::new()
                } else {
                    let local = crate::reglages::dossier_local();
                    crate::reseau::journee(&journal::lire_tout(&local), &journal::date())
                };
                self.reseau.rejouer(evenements, maintenant);
            }
        }
        // La ligne d'état dit ce que fait Eve quand aucun outil ne court.
        let repos = phrase_de_repos(self.champ("etat"));
        self.reseau.repos(repos, maintenant);
        // Le réseau vibre avec la voix (le niveau du micro tenu), et relit `sujet.js` chaque seconde.
        self.reseau.voix(self.etat.get("niveau").and_then(Value::as_f64).unwrap_or(0.0) as f32);
        if maintenant - self.sujet_lu >= 1.0 {
            self.sujet_lu = maintenant;
            let unix =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
            self.reseau.sujet_js(&self.centre.valeur("EVE_SUJET"), unix, maintenant);
            // La comptabilité que nomme le réglage : l'encaisse et les échéances fiscales n'ont qu'elle pour source.
            self.centre.suivre_la_comptabilite(self.champ("comptabilite"));
            // Ce qui est propre à la famille (famille.js) : ses personnes et les mots de l'école pour le réseau, quand ils
            // changent (le réseau remet ses éclairages à zéro) ; sa ville pour la météo, qui se relance quand elle change.
            let personnes = self.centre.personnes();
            if personnes != self.personnes_vues {
                self.reseau.personnes(personnes.clone());
                self.personnes_vues = personnes;
            }
            let motifs = self.centre.motifs_de_l_ecole();
            if motifs != self.motifs_vus {
                self.reseau.evoquer("ecole", motifs.clone());
                self.motifs_vus = motifs;
            }
            let lieu = meteo::lieu_de(&self.centre.famille());
            if lieu.as_ref() != self.meteo.as_ref().map(Meteo::lieu) {
                self.meteo = lieu.map(Meteo::lancer);
            }
            self.bulletin = self.meteo.as_ref().and_then(Meteo::bulletin);
            self.centre.tempete(self.bulletin.as_ref().map(|b| (b.neige_cm, b.rafales_kmh)));
            // Les services en ligne et Stripe vivent avec le thème « Constructo AI », le seul qui les montre : lancés
            // quand il vient, lâchés quand il s'en va (leurs fils s'arrêtent) ; leurs relevés relus ici, pas à chaque
            // image.
            if constructo != self.services.is_some() {
                self.services = constructo.then(Services::lancer);
                self.stripe = constructo.then(Stripe::lancer);
            }
            self.releve = self.services.as_ref().and_then(Services::releve);
            self.montants = self.stripe.as_ref().and_then(Stripe::montants);
            // La cueillette qui commence : les deux notes qui montent, comme la page (cdc:717).
            let cueillette = self.centre.cueillette(&journal::date(), &journal::heure());
            if cueillette
                && !self.cueillette
                && let Some(m) = self.musique.as_mut()
            {
                m.aller();
            }
            self.cueillette = cueillette;
        }
        // La musique suit Eve : elle se tait quand Eve écoute, transcrit ou parle ; plus vive quand elle travaille ;
        // plus basse la nuit (de 21 h à 6 h) ; sa gamme suit l'image du réseau ; et les accords des impulsions arrivées.
        // Le thème « Constructo AI », un thème de bureau comme l'ERP, est muet.
        let heure: u32 = journal::horodatage().get(11..13).and_then(|h| h.parse().ok()).unwrap_or(12);
        if let Some(m) = self.musique.as_mut() {
            m.regler(self.musique_allumee && !constructo, self.sons_allumes && !constructo);
            m.etat(self.etat.get("etat").and_then(Value::as_str).unwrap_or(""));
            m.nuit(!(6..21).contains(&heure));
            m.sujet(self.reseau.sujet_en_cours().unwrap_or("aucun"));
            while self.arrivees.front().is_some_and(|t| *t <= maintenant) {
                self.arrivees.pop_front();
                m.arrivee();
            }
        } else {
            self.arrivees.clear();
        }
        self.large = ui.max_rect().width() >= TABLEAU;
        self.hauteur = ui.max_rect().height();
        egui::Panel::top("barre")
            .exact_size(if self.cadre { BANDE + RANGEE } else { RANGEE })
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.barre(ui));
        let conversation = |s: &mut Fenetre, ui: &mut egui::Ui| {
            egui::Panel::top("tete").show_separator_line(false).frame(egui::Frame::new()).show(ui, |ui| s.tete(ui));
            egui::Panel::bottom("saisie")
                .show_separator_line(false)
                .frame(egui::Frame::new())
                .show(ui, |ui| s.saisie(ui));
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| s.fil(ui, Style::Bulles));
        };
        if constructo {
            self.constructo(ui);
        } else if self.centre_ouvert && self.large {
            // Le tableau : la conversation à droite, à la place du flux des entrées et sorties ; le centre de commande
            // sur le reste.
            egui::Panel::right("conversation")
                .resizable(true)
                .default_size(460.0)
                .size_range(380.0..=760.0)
                .frame(egui::Frame::new().fill(hud::FLUX).stroke((1.0, hud::BORD)).inner_margin(Margin {
                    left: 14,
                    right: 12,
                    top: 12,
                    bottom: 0,
                }))
                .show(ui, |ui| self.conversation_du_flux(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(Color32::BLACK))
                .show(ui, |ui| self.centre_hud(ui));
        } else {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(couleurs::SURFACE)).show(ui, |ui| {
                // Seule, la conversation garde une largeur de lecture, au milieu.
                let r = ui.max_rect();
                let colonne = egui::Rect::from_center_size(r.center(), vec2(r.width().min(800.0), r.height()));
                let colonne = colonne.shrink2(vec2(16.0, 0.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(colonne), |ui| conversation(self, ui));
            });
        }
        self.reglages(&ctx);
        self.historique(&ctx);
        self.depot(&ctx, if constructo { &pieces::CLAIR } else { &pieces::SOMBRE });
        if self.cadre {
            decrire_la_barre(&ctx, BANDE + RANGEE, &self.zones_de_la_barre, &self.boutons_de_fenetre);
        }
        // Une image est dessinée : la fenêtre est ouverte, pour le chef.
        self.presence.arriver();
        // L'orbe demande elle-même ses images (30 par seconde, 15 en transcription) ; la fenêtre ajoute les 30 du réseau
        // qui s'anime et du micro tenu, et deux par seconde au moins (l'horloge du tableau).
        let vif = self.anime || self.parle || !self.arrivees.is_empty();
        ctx.request_repaint_after(Duration::from_millis(if vif { 33 } else { 500 }));
    }

    /// Les ressources d'OpenGL de l'orbe se rendent tant que le contexte vit encore.
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(gl) = gl {
            self.orbe.liberer(gl);
        }
    }
}

// ---- Le style ---------------------------------------------------------------------------------------------------

/// Du texte en Segoe UI Semibold.
pub(crate) fn semi(texte: impl Into<String>, taille: f32) -> RichText {
    RichText::new(texte).family(FontFamily::Name(SEMI.into())).size(taille)
}

/// Une icône de Windows.
pub(crate) fn icone(code: char, taille: f32) -> RichText {
    RichText::new(code.to_string()).family(FontFamily::Name(ICONES.into())).size(taille)
}

/// Les sortes de boutons : plat (un fond au survol), plein (l'action principale), cerné (l'autre choix), et celui de
/// la barre (allumé quand ce qu'il commande est ouvert).
#[derive(Clone, Copy)]
pub(crate) enum Bouton {
    Plat,
    Plein(Color32),
    Cerne,
    Barre(bool),
}

/// Un bouton dessiné à la main : une icône, au besoin, et un mot.
pub(crate) fn bouton(ui: &mut egui::Ui, code: Option<char>, texte: &str, sorte: Bouton) -> egui::Response {
    let petit = matches!(sorte, Bouton::Plat | Bouton::Barre(_));
    let taille = if petit { 12.5 } else { 13.0 };
    let peintre = ui.painter();
    let g_texte = peintre.layout_no_wrap(
        texte.to_string(),
        FontId::new(taille, FontFamily::Name(SEMI.into())),
        Color32::PLACEHOLDER,
    );
    let g_icone = code.map(|c| {
        peintre.layout_no_wrap(
            c.to_string(),
            FontId::new(taille, FontFamily::Name(ICONES.into())),
            Color32::PLACEHOLDER,
        )
    });
    let (px, py) = if petit { (9.0, 5.0) } else { (14.0, 7.0) };
    let l_icone = g_icone.as_ref().map_or(0.0, |g| g.size().x + 7.0);
    let taille_bouton = vec2(2.0 * px + l_icone + g_texte.size().x, 2.0 * py + g_texte.size().y);
    let (rect, rep) = ui.allocate_exact_size(taille_bouton, Sense::click());
    if ui.is_rect_visible(rect) {
        let (survol, enfonce) = (rep.hovered(), rep.is_pointer_button_down_on());
        let rien = Color32::TRANSPARENT;
        let pan = panneau(ui);
        let (fond, bord, encre) = match sorte {
            Bouton::Plat => {
                (if survol { pan.survol } else { rien }, rien, if survol { pan.encre } else { pan.encre_2 })
            }
            Bouton::Plein(c) => (
                match (enfonce, survol) {
                    (true, _) => melange(c, Color32::BLACK, 0.15),
                    (false, true) => melange(c, Color32::WHITE, 0.12),
                    _ => c,
                },
                rien,
                pan.sur_plein,
            ),
            Bouton::Cerne => (if survol { pan.survol } else { rien }, pan.filet_fort, pan.encre),
            Bouton::Barre(actif) => (
                match (actif, survol) {
                    (_, true) => Color32::from_white_alpha(30),
                    (true, false) => Color32::from_white_alpha(20),
                    _ => rien,
                },
                if actif { Color32::from_white_alpha(36) } else { rien },
                if actif || survol { Color32::WHITE } else { couleurs::BARRE_TEXTE },
            ),
        };
        let p = ui.painter();
        p.rect(rect, 8.0, fond, Stroke::new(1.0, bord), egui::StrokeKind::Inside);
        let mut x = rect.left() + px;
        if let Some(g) = g_icone {
            let l = g.size().x;
            p.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, encre);
            x += l + 7.0;
        }
        p.galley(egui::pos2(x, rect.center().y - g_texte.size().y / 2.0), g_texte, encre);
    }
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Une valeur changée dans les réglages reste affichée ce temps-là, le temps que le chef la publie (pas de clignotement).
const TENUE: f64 = 4.0;

/// Le champ de la saisie grandit jusqu'à ce nombre de lignes ; au-delà, il défile.
const LIGNES_DE_LA_SAISIE: f32 = 6.0;

/// L'allure du champ de la saisie dans un thème ([`Fenetre::champ_de_saisie`]) : sa police, son encre (sans elle, celle
/// des visuels), son indice, sa marge, sa largeur.
pub(crate) struct AllureDuChamp {
    pub(crate) police: FontId,
    pub(crate) encre: Option<Color32>,
    pub(crate) indice: egui::WidgetText,
    pub(crate) marge: Margin,
    pub(crate) largeur: f32,
}

/// La valeur d'un réglage : celle changée ici depuis moins de [`TENUE`], sinon celle que publie le chef
/// (`reglages.<clé>`, `reglages.alertes.<sorte>` ; le mot d'éveil, à défaut, `eveil`) ; `None` s'il n'en dit rien.
fn valeur_reglage(etat: &Value, tenus: &HashMap<String, (bool, f64)>, cle: &str, maintenant: f64) -> Option<bool> {
    if let Some(&(v, t)) = tenus.get(cle)
        && maintenant - t < TENUE
    {
        return Some(v);
    }
    let reglages = &etat["reglages"];
    let v = match cle.split_once('.') {
        Some((groupe, nom)) => &reglages[groupe][nom],
        None => &reglages[cle],
    };
    v.as_bool().or_else(|| if cle == "eveil" { etat["eveil"].as_bool() } else { None })
}

/// Le titre d'une rubrique des réglages (la page : `.rubrique`) : petit, en capitales, espacé.
fn rubrique(ui: &mut egui::Ui, texte: &str) {
    ui.add_space(12.0);
    let titre = RichText::new(texte.to_uppercase()).family(FontFamily::Name(GRAS.into())).size(11.0);
    ui.label(titre.extra_letter_spacing(1.0).color(panneau(ui).encre_3));
    ui.add_space(5.0);
}

/// Le style d'une liste qui s'ouvre d'un panneau (celle des voix) : sur sa propre couche, elle prendrait les visuels de
/// la fenêtre ; dans le thème clair, elle prend ceux de son panneau.
fn style_de_la_liste(ui: &egui::Ui) -> egui::style::StyleModifier {
    if ui.visuals().dark_mode {
        egui::style::StyleModifier::default()
    } else {
        egui::style::StyleModifier::from(|s: &mut egui::Style| theme::visuels_du_panneau(&mut s.visuals))
    }
}

/// Un choix en segments, comme la page (`.segments`, eve.html : 415-427), dans les couleurs du panneau des réglages :
/// des cases de même largeur dans un cadre, celle qui est choisie en pilule, avec son ombre et son filet (aucune, si
/// `choisi` n'en est pas une). `id` le distingue des autres choix du panneau. Rend la case cliquée.
fn segments(ui: &mut egui::Ui, id: &str, choix: &[&str], choisi: usize) -> Option<usize> {
    let pan = panneau(ui);
    let (cadre, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let filet = Stroke::new(1.0, pan.filet);
    ui.painter().rect(cadre, 11.0, pan.piste, filet, egui::StrokeKind::Inside);
    let n = choix.len().max(1) as f32;
    let dedans = cadre.shrink(4.0);
    let l = (dedans.width() - 3.0 * (n - 1.0)) / n;
    let police = FontId::new(13.5, FontFamily::Name(SEMI.into()));
    let mut clic = None;
    for (k, mot) in choix.iter().enumerate() {
        let coin = egui::pos2(dedans.left() + k as f32 * (l + 3.0), dedans.top());
        let r = egui::Rect::from_min_size(coin, vec2(l, dedans.height()));
        let rep = ui.interact(r, egui::Id::unique(("segment", id, k)), Sense::click());
        let p = ui.painter();
        if k == choisi {
            let ombre = egui::Shadow { offset: [0, 1], blur: 2, spread: 0, color: pan.ombres[0].color };
            p.add(ombre.as_shape(r, 8));
            p.rect(r, 8.0, pan.choisi, filet, egui::StrokeKind::Outside);
        }
        let encre = if k == choisi || rep.hovered() { pan.encre } else { pan.encre_2 };
        p.text(r.center(), Align2::CENTER_CENTER, *mot, police.clone(), encre);
        if rep.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clic = Some(k);
        }
    }
    clic
}

/// Un groupe de rangées des réglages (la page : `.groupe`) : un fond un peu plus clair, un filet, des coins arrondis.
fn groupe(ui: &mut egui::Ui, contenu: impl FnOnce(&mut egui::Ui)) {
    let p = panneau(ui);
    egui::Frame::new()
        .fill(p.groupe)
        .stroke((1.0, p.filet))
        .corner_radius(11)
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contenu(ui);
        });
}

/// Une rangée d'interrupteur (la page : `.inter`) : le libellé à gauche, à la ligne au besoin, la pilule de 40 sur 24
/// à droite, l'accent quand elle est mise, son rond blanc qui glisse en 0,18 s ; un filet au-dessus, sauf la première.
/// Toute la rangée se clique ; sans valeur, elle est grisée.
fn rangee_interrupteur(
    ui: &mut egui::Ui,
    texte: &str,
    valeur: Option<bool>,
    premier: bool,
    id: egui::Id,
) -> egui::Response {
    let largeur = ui.available_width();
    let pan = panneau(ui);
    let encre = if valeur.is_some() { pan.encre } else { pan.encre_3 };
    let galley = ui.painter().layout(texte.to_string(), FontId::proportional(14.0), encre, largeur - 40.0 - 14.0);
    let hauteur = (galley.size().y + 10.0).max(34.0);
    let sens = if valeur.is_some() { Sense::click() } else { Sense::hover() };
    let (rect, rep) = ui.allocate_exact_size(vec2(largeur, hauteur), sens);
    let p = ui.painter();
    if !premier {
        p.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, pan.filet));
    }
    p.galley(egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, encre);
    let t = ui.ctx().animate_bool_with_time(id, valeur.unwrap_or(false), 0.18);
    let opacite = if valeur.is_some() { 1.0 } else { 0.45 };
    let pilule = egui::Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), vec2(40.0, 24.0));
    p.rect_filled(pilule, 12.0, melange(pan.filet_fort, pan.accent, t).gamma_multiply(opacite));
    let x = egui::lerp(pilule.left() + 12.0..=pilule.right() - 12.0, t);
    p.circle_filled(egui::pos2(x, pilule.center().y), 8.5, Color32::WHITE.gamma_multiply(opacite));
    if valeur.is_some() { rep.on_hover_cursor(egui::CursorIcon::PointingHand) } else { rep }
}

/// Une touche du clavier, dessinée comme une touche : un petit cadre, son nom.
fn touche_de_clavier(ui: &mut egui::Ui, nom: &str) {
    let p = panneau(ui);
    egui::Frame::new()
        .fill(p.groupe)
        .stroke((1.0, p.filet_fort))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| ui.label(semi(nom, 11.5).color(p.encre)));
}

/// Le bouton qui ferme un panneau : un × de 32 sur 32, un fond et un filet au survol.
fn bouton_fermer(ui: &mut egui::Ui) -> egui::Response {
    let pan = panneau(ui);
    let (rect, rep) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let survol = rep.hovered();
    let p = ui.painter();
    if survol {
        p.rect(rect, 8.0, pan.groupe, Stroke::new(1.0, pan.filet), egui::StrokeKind::Inside);
    }
    let encre = if survol { pan.encre } else { pan.encre_2 };
    let icone = FontId::new(12.0, FontFamily::Name(ICONES.into()));
    p.text(rect.center(), Align2::CENTER_CENTER, icones::NON, icone, encre);
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// « Quitter Eve », au bas des réglages : toute la largeur, en rouge ; au survol, un filet et un fond rouges.
fn bouton_quitter(ui: &mut egui::Ui) -> egui::Response {
    let pan = panneau(ui);
    let (rect, rep) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let survol = rep.hovered();
    let (fond, bord) =
        if survol { (pan.crit.gamma_multiply(0.10), pan.crit) } else { (Color32::TRANSPARENT, pan.filet_fort) };
    let p = ui.painter();
    p.rect(rect, 9.0, fond, Stroke::new(1.0, bord), egui::StrokeKind::Inside);
    let police = FontId::new(13.0, FontFamily::Name(SEMI.into()));
    p.text(rect.center(), Align2::CENTER_CENTER, "Quitter Eve", police, pan.crit);
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// L'envoi : un rond, de l'accent quand il y a quelque chose à envoyer.
fn bouton_envoyer(ui: &mut egui::Ui, pret: bool) -> egui::Response {
    let (rect, rep) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    let fond = match (pret, rep.hovered()) {
        (false, _) => couleurs::SURFACE_2,
        (true, true) => melange(couleurs::ACCENT, Color32::WHITE, 0.12),
        (true, false) => couleurs::ACCENT,
    };
    let encre = if pret { couleurs::SUR_PLEIN } else { couleurs::ENCRE_3 };
    let p = ui.painter();
    p.circle_filled(rect.center(), 17.0, fond);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icones::ENVOYER,
        FontId::new(15.0, FontFamily::Name(ICONES.into())),
        encre,
    );
    rep.on_hover_text(AIDE_DE_L_ENVOI).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// La bulle du bouton d'envoi, dans les deux thèmes.
pub(crate) const AIDE_DE_L_ENVOI: &str = "Envoyer (Entrée) · Maj+Entrée : à la ligne · ↑ : reprendre une demande";

/// Le bouton qui ramène au bas de la conversation : un rond, sa flèche vers le bas, dans les couleurs du fil — celles
/// d'un panneau (sombre ou clair, son accent pour la flèche), ou celles du tableau HTML dans le flux ; une ombre, plus
/// marquée au survol ; `montre` : son fondu (0 à 1).
fn bouton_du_bas(ui: &egui::Ui, rect: egui::Rect, survol: bool, montre: f32, style: Style) {
    let p = panneau(ui);
    let (fond, filet, encre) =
        if style == Style::Flux { (hud::FLUX, hud::BORD, hud::CYAN) } else { (p.fond, p.filet_fort, p.accent) };
    let peintre = ui.painter();
    let ombre = egui::Shadow {
        offset: [0, if survol { 4 } else { 2 }],
        blur: if survol { 14 } else { 8 },
        spread: 0,
        color: p.ombres[0].color.gamma_multiply(montre),
    };
    peintre.add(ombre.as_shape(rect, 19));
    let fond = if survol { melange(fond, encre, 0.08) } else { fond };
    peintre.circle(rect.center(), 19.0, fond.gamma_multiply(montre), Stroke::new(1.0, filet.gamma_multiply(montre)));
    let fleche = FontId::new(15.0, FontFamily::Name(ICONES.into()));
    peintre.text(rect.center(), Align2::CENTER_CENTER, icones::EN_BAS, fleche, encre.gamma_multiply(montre));
}

/// Une pastille d'état de la barre : un point de couleur et son nom. Rend sa place.
fn pastille(ui: &mut egui::Ui, nom: &str, couleur: Color32) -> egui::Rect {
    egui::Frame::new()
        .fill(Color32::from_white_alpha(14))
        .stroke((1.0, Color32::from_white_alpha(20)))
        .corner_radius(255)
        .inner_margin(Margin { left: 8, right: 10, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (point, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                ui.painter().circle_filled(point.center(), 3.5, couleur);
                ui.label(RichText::new(nom).size(11.5).color(couleurs::BARRE_TEXTE));
            });
        })
        .response
        .rect
}

/// Comment se montre un bouton de la fenêtre : survolé, enfoncé, la fenêtre inactive ; et pour « agrandir », la fenêtre
/// déjà agrandie (il la restaure).
#[derive(Clone, Copy, Default)]
pub(crate) struct Allure {
    pub survol: bool,
    pub enfonce: bool,
    pub inactive: bool,
    pub agrandie: bool,
}

/// Un bouton de la fenêtre, comme ceux de Windows 11 : son glyphe de Segoe Fluent Icons au milieu, à 10 points ; au
/// survol, un voile clair, ou le rouge de Windows pour « fermer », et le glyphe blanc ; enfoncé, un peu moins clair ;
/// la fenêtre inactive, un glyphe plus pâle.
pub(crate) fn bouton_de_fenetre(p: &egui::Painter, rect: egui::Rect, bouton: cadre::Bouton, allure: Allure) {
    let fermer = bouton == cadre::Bouton::Fermer;
    let fond = match (fermer, allure.enfonce, allure.survol) {
        (true, true, _) => melange(couleurs::FERMER, couleurs::BARRE_HAUT, 0.1),
        (true, false, true) => couleurs::FERMER,
        (false, true, _) => Color32::from_white_alpha(16),
        (false, false, true) => Color32::from_white_alpha(26),
        _ => Color32::TRANSPARENT,
    };
    p.rect_filled(rect, 0.0, fond);
    let glyphe = match bouton {
        cadre::Bouton::Reduire => icones::REDUIRE,
        cadre::Bouton::Agrandir if allure.agrandie => icones::RESTAURER,
        cadre::Bouton::Agrandir => icones::AGRANDIR,
        cadre::Bouton::Fermer => icones::FERMER,
    };
    let encre = if allure.survol || allure.enfonce {
        Color32::WHITE
    } else if allure.inactive {
        couleurs::BARRE_TEXTE.gamma_multiply(0.55)
    } else {
        couleurs::BARRE_CLAIR
    };
    p.text(rect.center(), Align2::CENTER_CENTER, glyphe, FontId::new(10.0, FontFamily::Name(ICONES.into())), encre);
}

/// La bande du haut de la barre, à la place de la barre de titre de Windows : le site de Constructo AI au-dessus du logo
/// (`gauche`, le bord du logo), petit et discret, dans la couleur des textes de la barre ; survolé, il s'éclaire et se
/// souligne ; un clic l'ouvre dans le navigateur (le geste `site`). À droite, les boutons de la fenêtre, sauf en plein
/// écran (Windows n'en montre pas non plus) ; leur survol et leur clic, Windows les dit à [`cadre`]. Rend les boutons
/// dessinés et leur place, et la réponse du lien.
fn bande_du_haut(ui: &egui::Ui, bande: egui::Rect, gauche: f32) -> (Vec<(cadre::Bouton, egui::Rect)>, egui::Response) {
    let p = ui.painter();
    let (plein_ecran, agrandie, active) = ui.input(|i| {
        let v = i.viewport();
        (v.fullscreen.unwrap_or(false), v.maximized.unwrap_or(false), v.focused.unwrap_or(true))
    });
    let boutons: Vec<(cadre::Bouton, egui::Rect)> = if plein_ecran {
        Vec::new()
    } else {
        [cadre::Bouton::Fermer, cadre::Bouton::Agrandir, cadre::Bouton::Reduire]
            .into_iter()
            .zip(0..)
            .map(|(b, i)| {
                let droite = bande.right() - i as f32 * BOUTON_DE_FENETRE;
                let r = egui::Rect::from_min_max(
                    egui::pos2(droite - BOUTON_DE_FENETRE, bande.top()),
                    egui::pos2(droite, bande.bottom()),
                );
                (b, r)
            })
            .collect()
    };
    let (survol, enfonce) = (cadre::survole(), cadre::enfonce());
    for &(b, r) in &boutons {
        // Un bouton enfoncé ne paraît enfoncé que survolé ; pendant ce temps, les autres ne s'allument pas.
        let allure = Allure {
            survol: survol == Some(b) && enfonce.is_none_or(|e| e == b),
            enfonce: enfonce == Some(b) && survol == Some(b),
            inactive: !active,
            agrandie,
        };
        bouton_de_fenetre(p, r, b, allure);
    }
    let fin = boutons.last().map_or(bande.right(), |(_, r)| r.left()) - 16.0;
    // La couleur se donne au dessin : elle change au survol.
    let format = TextFormat {
        font_id: FontId::proportional(11.5),
        color: Color32::PLACEHOLDER,
        extra_letter_spacing: 0.2,
        ..Default::default()
    };
    let mut job = LayoutJob::single_section(crate::connexion::SITE.to_string(), format);
    job.wrap = egui::text::TextWrapping {
        max_width: (fin - gauche).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let g = p.layout_job(job);
    let coin = egui::pos2(gauche, bande.center().y - g.size().y / 2.0);
    let texte = egui::Rect::from_min_size(coin, g.size());
    let site = ui
        .interact(texte.expand2(vec2(4.0, 3.0)), egui::Id::unique("site-de-constructo-ai"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Ouvrir le site de Constructo AI");
    let encre = if site.hovered() { couleurs::BARRE_CLAIR } else { couleurs::BARRE_TEXTE };
    p.galley(coin, g, encre);
    if site.hovered() {
        p.hline(texte.x_range(), texte.bottom() - 1.0, Stroke::new(1.0, encre));
    }
    // La révision d'Eve, à droite du site (« v7 » : `Cargo.toml`, sa seule source) ; elle ne se clique pas.
    let police = FontId::new(11.5, FontFamily::Name(SEMI.into()));
    let revision = p.layout_no_wrap(crate::mise_a_jour::revision(), police, couleurs::BARRE_TEXTE);
    let x = texte.right() + 10.0;
    if x + revision.size().x < fin {
        p.galley(egui::pos2(x, bande.center().y - revision.size().y / 2.0), revision, couleurs::BARRE_TEXTE);
    }
    (boutons, site)
}

/// La barre décrite à Windows ([`cadre::decrire`]), à chaque image : sa hauteur ; ce qui s'y clique dans egui — les
/// boutons et les pastilles de la barre (`client`), et ce qui s'ouvre par-dessus, un panneau, une liste ; les boutons
/// de la fenêtre ; l'échelle.
fn decrire_la_barre(ctx: &egui::Context, hauteur: f32, client: &[egui::Rect], boutons: &[(cadre::Bouton, egui::Rect)]) {
    let mut client = client.to_vec();
    ctx.memory(|m| {
        for couche in m.areas().visible_layer_ids() {
            if matches!(couche.order, egui::Order::Middle | egui::Order::Foreground)
                && let Some(r) = m.area_rect(couche.id)
            {
                client.push(r);
            }
        }
    });
    cadre::decrire(cadre::Barre { hauteur, client, boutons: boutons.to_vec(), echelle: ctx.pixels_per_point() });
}

/// Segoe UI (celle de Windows, que prend l'Eve de Constructo AI), sa graisse et sa demi-graisse, Segoe UI Symbol pour
/// les signes qu'elle n'a pas, les icônes de Windows, Cascadia Mono : ce que le poste a ; sinon, les polices d'egui.
pub(crate) fn polices(ctx: &egui::Context) {
    let mut p = egui::FontDefinitions::default();
    let lire = |nom: &str| std::fs::read(format!(r"C:\Windows\Fonts\{nom}")).ok();
    let mut charger = |cle: &str, fichiers: &[&str]| -> Option<String> {
        let octets = fichiers.iter().find_map(|f| lire(f))?;
        p.font_data.insert(cle.into(), Arc::new(egui::FontData::from_owned(octets)));
        Some(cle.to_string())
    };
    let segoe = charger("segoe", &["segoeui.ttf"]);
    let symboles = charger("segoe-symboles", &["seguisym.ttf"]);
    let demi = charger("segoe-semi", &["seguisb.ttf"]);
    let gras = charger("segoe-gras", &["segoeuib.ttf"]);
    let pictos = charger("segoe-icones", &["SegoeIcons.ttf", "segmdl2.ttf"]);
    // Consolas d'abord : la police du tableau HTML de l'Assistante famille.
    let mono = charger("mono", &["consola.ttf", "CascadiaMono.ttf"]);
    // Chaque famille : sa police, les symboles, puis les polices d'egui (les emojis compris).
    let base = p.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let famille = |tete: &[&Option<String>], reste: &[String]| -> Vec<String> {
        tete.iter().filter_map(|x| (*x).clone()).chain(reste.iter().cloned()).collect()
    };
    p.families.insert(FontFamily::Proportional, famille(&[&segoe, &symboles], &base));
    p.families.insert(FontFamily::Name(SEMI.into()), famille(&[&demi, &segoe, &symboles], &base));
    p.families.insert(FontFamily::Name(GRAS.into()), famille(&[&gras, &demi, &segoe], &base));
    p.families.insert(FontFamily::Name(ICONES.into()), famille(&[&pictos, &symboles], &base));
    let base_mono = p.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    p.families.insert(FontFamily::Monospace, famille(&[&mono, &symboles], &base_mono));
    ctx.set_fonts(p);
}

/// Le thème sombre de Constructo AI : ses couleurs, ses coins arrondis, ses espacements, ses tailles de texte.
pub(crate) fn apparence(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = couleurs::FOND;
    v.window_fill = couleurs::SURFACE;
    v.extreme_bg_color = couleurs::FOND;
    v.faint_bg_color = couleurs::SURFACE_2;
    v.hyperlink_color = couleurs::ACCENT;
    v.selection.bg_fill = couleurs::ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, couleurs::ACCENT);
    v.text_cursor.stroke = Stroke::new(2.0, couleurs::ACCENT);
    v.window_corner_radius = CornerRadius::same(12);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, couleurs::FILET);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, couleurs::ENCRE);
    v.widgets.inactive.weak_bg_fill = couleurs::PILULE;
    v.widgets.inactive.bg_fill = couleurs::PILULE;
    v.widgets.hovered.weak_bg_fill = couleurs::FILET_FORT;
    v.widgets.active.weak_bg_fill = couleurs::ACCENT.gamma_multiply(0.6);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        // Les barres de défilement flottent aux couleurs des panneaux (pas au blanc du texte), à 10 points du contenu.
        s.spacing.scroll.foreground_color = false;
        s.spacing.scroll.content_margin = Margin { right: 10, ..Margin::ZERO };
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.5));
    });
}

/// Le mélange de deux couleurs : `t` = 0 donne la première, 1 la seconde.
pub(crate) fn melange(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// ---- La fenêtre de Windows ----------------------------------------------------------------------------------------

/// La taille de la fenêtre au départ : 82 % de la largeur de l'écran et 86 % de sa hauteur (la barre des tâches
/// ôtée), dans des bornes raisonnables, et centrée ; sans écran connu, 1240 × 780.
fn place_initiale() -> ([f32; 2], Option<[f32; 2]>) {
    #[cfg(windows)]
    if let Some((x, y, l, h)) = zone_de_travail() {
        let taille = [(l * 0.82).clamp(960.0, 1560.0).min(l), (h * 0.86).clamp(600.0, 1000.0).min(h)];
        return (taille, Some([x + (l - taille[0]) / 2.0, y + (h - taille[1]) / 2.0]));
    }
    ([1240.0, 780.0], None)
}

/// La zone de travail de l'écran principal (sans la barre des tâches), en points : à ce moment, le programme ne
/// s'est pas encore dit sensible à l'échelle de l'écran, et Windows lui donne des pixels à 96 points par pouce — le
/// rapport à `GetDpiForSystem` le vaut dans les deux cas.
#[cfg(windows)]
fn zone_de_travail() -> Option<(f32, f32, f32, f32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut r = RECT::default();
    let pointeur = (&raw mut r).cast::<core::ffi::c_void>();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(pointeur), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }
        .ok()?;
    let echelle = match unsafe { GetDpiForSystem() } {
        0 => 1.0,
        dpi => dpi as f32 / 96.0,
    };
    let (l, h) = ((r.right - r.left) as f32 / echelle, (r.bottom - r.top) as f32 / echelle);
    (l > 200.0 && h > 200.0).then_some((r.left as f32 / echelle, r.top as f32 / echelle, l, h))
}

/// La barre de titre de la fenêtre devient celle d'Eve ([`cadre`]) : Windows garde son cadre, la barre d'Eve monte
/// tout en haut avec ses boutons de fenêtre. Avant, ce qui vaut si Windows refusait (sa barre resterait) : elle prend
/// la couleur de la barre d'Eve, marine, avec un texte clair (Windows 11 ; Windows 10 ignore la couleur et garde une
/// barre sombre), sans titre ni icône ; le liseré de la fenêtre prend le même marine. `true` : la barre est celle
/// d'Eve.
#[cfg(windows)]
fn barre_de_titre(cc: &eframe::CreationContext<'_>) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
        DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Controls::{
        SetWindowThemeAttribute, WTA_NONCLIENT, WTA_OPTIONS, WTNCA_NODRAWCAPTION, WTNCA_NODRAWICON,
    };
    let Ok(poignee) = cc.window_handle() else { return false };
    let RawWindowHandle::Win32(w) = poignee.as_raw() else { return false };
    let hwnd = HWND(w.hwnd.get() as *mut core::ffi::c_void);
    // Une COLORREF : 0x00BBGGRR.
    let colorref = |c: Color32| u32::from(c.r()) | (u32::from(c.g()) << 8) | (u32::from(c.b()) << 16);
    let poser = |attribut: DWMWINDOWATTRIBUTE, valeur: u32| {
        let _ = unsafe { DwmSetWindowAttribute(hwnd, attribut, (&raw const valeur).cast(), 4) };
    };
    poser(DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    poser(DWMWA_CAPTION_COLOR, colorref(couleurs::BARRE_HAUT));
    poser(DWMWA_BORDER_COLOR, colorref(couleurs::BARRE_HAUT));
    poser(DWMWA_TEXT_COLOR, colorref(couleurs::BARRE_CLAIR));
    // La barre d'Eve, juste dessous, porte déjà le logo et le nom : la barre de Windows ne les répète pas. La fenêtre
    // garde son titre, pour la barre des tâches, Alt+Tab et Ctrl+Alt+E.
    let sans = WTNCA_NODRAWCAPTION | WTNCA_NODRAWICON;
    let options = WTA_OPTIONS { dwFlags: sans, dwMask: sans };
    // SAFETY : `hwnd` est la fenêtre ouverte ; `options` vit jusqu'au retour, et sa taille est celle qu'on annonce.
    let _ = unsafe {
        SetWindowThemeAttribute(hwnd, WTA_NONCLIENT, (&raw const options).cast(), size_of::<WTA_OPTIONS>() as u32)
    };
    // Un bouton de la fenêtre survolé ou enfoncé : Windows le dit au sous-classement, et la fenêtre se redessine.
    let ctx = cc.egui_ctx.clone();
    let ok = cadre::installer(hwnd, Box::new(move || ctx.request_repaint()));
    if !ok {
        journal::ecrire("La barre de titre reste celle de Windows : le sous-classement de la fenêtre est refusé.");
    }
    ok
}

/// L'icône de la fenêtre en PNG : celle des notifications de Windows (`notifications.rs`).
pub(crate) fn png_de_l_icone() -> Option<Vec<u8>> {
    let icone = icone_de_fenetre()?;
    crate::cartes::png_rgba(icone.width, icone.height, &icone.rgba).ok()
}

/// L'icône de la fenêtre et de la barre des tâches : le logo blanc sur un carré marine aux coins arrondis.
fn icone_de_fenetre() -> Option<egui::IconData> {
    const N: usize = 64;
    let logo = logo_blanc(255)?;
    let [lw, lh] = logo.size;
    let (rayon, milieu, cote) = (14.0f32, N as f32 / 2.0, 44.0f32);
    let mut rgba = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Le carré arrondi, lissé sur un pixel.
            let qx = ((fx - milieu).abs() - (milieu - rayon)).max(0.0);
            let qy = ((fy - milieu).abs() - (milieu - rayon)).max(0.0);
            let fond = (rayon + 0.5 - (qx * qx + qy * qy).sqrt()).clamp(0.0, 1.0);
            // Le logo au milieu, réduit à 44 pixels.
            let u = (fx - (milieu - cote / 2.0)) / cote * lw as f32;
            let v = (fy - (milieu - cote / 2.0)) / cote * lh as f32;
            let a = alpha_bilineaire(&logo, u, v);
            let m = |b: u8| (f32::from(b) * (1.0 - a) + 255.0 * a).round() as u8;
            let i = (y * N + x) * 4;
            let c = couleurs::BARRE_HAUT;
            rgba[i..i + 4].copy_from_slice(&[m(c.r()), m(c.g()), m(c.b()), (fond * 255.0).round() as u8]);
        }
    }
    Some(egui::IconData { rgba, width: N as u32, height: N as u32 })
}

/// La transparence d'une image au point (`u`, `v`), en pixels, interpolée entre ses quatre voisins.
fn alpha_bilineaire(image: &egui::ColorImage, u: f32, v: f32) -> f32 {
    let [l, h] = image.size;
    let a = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= l as i64 || y >= h as i64 {
            0.0
        } else {
            f32::from(image.pixels[y as usize * l + x as usize].a()) / 255.0
        }
    };
    let (x, y) = (u - 0.5, v - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let haut = a(x0, y0) * (1.0 - tx) + a(x0 + 1, y0) * tx;
    let bas = a(x0, y0 + 1) * (1.0 - tx) + a(x0 + 1, y0 + 1) * tx;
    haut * (1.0 - ty) + bas * ty
}

/// Le logo en blanc (comme `filter: brightness(0) invert(1)` dans la page), sa transparence gardée, à `opacite` sur
/// 255.
fn logo_blanc(opacite: u16) -> Option<egui::ColorImage> {
    let mut lecteur = png::Decoder::new(std::io::Cursor::new(LOGO)).read_info().ok()?;
    let mut octets = vec![0; lecteur.output_buffer_size()?];
    let info = lecteur.next_frame(&mut octets).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let mut rgba = octets[..info.buffer_size()].to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        px[3] = (u16::from(px[3]) * opacite / 255) as u8;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &rgba))
}

/// Un dégradé vertical, de `haut` en haut à `bas` en bas.
fn degrade(p: &egui::Painter, rect: egui::Rect, haut: Color32, bas: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), haut);
    m.colored_vertex(rect.right_top(), haut);
    m.colored_vertex(rect.right_bottom(), bas);
    m.colored_vertex(rect.left_bottom(), bas);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// La couleur de l'orbe à la distance `f` du reflet (0, le reflet ; 1, le bord), entre les arrêts de son dégradé
/// (0 %, 42 %, 72 %, 100 %).
fn degrade_orbe(f: f32) -> Color32 {
    let arrets = [0.0, 0.42, 0.72, 1.0];
    let i = arrets.windows(2).position(|w| f <= w[1]).unwrap_or(2);
    let t = ((f - arrets[i]) / (arrets[i + 1] - arrets[i])).clamp(0.0, 1.0);
    melange(couleurs::ORBE[i], couleurs::ORBE[i + 1], t)
}

/// La sphère de l'orbe : des disques de plus en plus petits, du bord au reflet (en haut à gauche).
fn sphere(p: &egui::Painter, c: egui::Pos2, r: f32) {
    let reflet = c + vec2(-0.28 * r, -0.36 * r);
    let pas = 28;
    for i in 0..pas {
        let f = i as f32 / pas as f32;
        p.circle_filled(c + (reflet - c) * (f * 0.9), r * (1.0 - f * 0.92), degrade_orbe(1.0 - f));
    }
}

// ---- Le fil -------------------------------------------------------------------------------------------------------

/// Une ligne du fil, telle que l'état la publie. `ancien` : d'une séance précédente ; `en_cours` : Eve l'écrit encore.
pub(crate) struct Ligne<'a> {
    pub(crate) de: &'a str,
    pub(crate) texte: &'a str,
    pub(crate) heure: &'a str,
    pub(crate) ancien: bool,
    pub(crate) pieces: &'a [&'a str],
    pub(crate) en_cours: bool,
    /// Sous une réponse de Claude : ce que son tour a pris (« 12 s · 0,04 $ · Sonnet 5.5, moyen ») ; vide ailleurs.
    pub(crate) tour: &'a str,
}

/// Une ligne du fil en bulle, dans le thème de son `Ui` : claire ([`theme::bulle`]) dans un `Ui` aux visuels clairs
/// (l'historique ouvert dans le thème « Constructo AI »), sombre sinon ([`bulle`]).
pub(crate) fn bulle_du_theme(ui: &mut egui::Ui, l: &Ligne<'_>, cartes: &mut Cartes, gestes: &mut Vec<Geste>) {
    if ui.visuals().dark_mode { bulle(ui, l, cartes, gestes) } else { theme::bulle(ui, l, cartes, gestes) }
}

/// Une ligne du fil : la personne à droite, Eve à gauche (avec sa petite orbe), Eve-même (le système) au centre, une
/// alerte en ambre ; une ligne d'une séance précédente, plus pâle. Les cartes d'Eve s'y dessinent ([`Cartes`]) ; les
/// gestes qu'on y demande s'ajoutent à `gestes`.
pub(crate) fn bulle(ui: &mut egui::Ui, l: &Ligne<'_>, cartes: &mut Cartes, gestes: &mut Vec<Geste>) {
    let Ligne { de, texte, heure, ancien, pieces, en_cours, tour } = *l;
    let opacite = if ancien { 0.55 } else { 1.0 };
    let pale = |c: Color32| c.gamma_multiply(opacite);
    let max = (ui.available_width() * 0.84).min(620.0);
    let marge = Margin { left: 12, right: 12, top: 8, bottom: 9 };
    match de {
        "systeme" => {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width() * 0.9);
                ui.label(RichText::new(texte).size(11.5).color(pale(couleurs::ENCRE_3)));
            });
            ui.add_space(8.0);
        }
        "toi" => {
            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                egui::Frame::new()
                    .fill(pale(couleurs::BULLE_TOI))
                    .stroke((1.0, pale(couleurs::BULLE_TOI_BORD)))
                    .corner_radius(CornerRadius { nw: 14, ne: 14, sw: 14, se: 4 })
                    .inner_margin(marge)
                    .show(ui, |ui| {
                        ui.set_max_width(max);
                        ui.label(RichText::new(texte).size(14.0).color(pale(couleurs::ENCRE)));
                        pieces_jointes(ui, pieces, opacite);
                    });
                heure_sous(ui, heure, opacite);
            });
        }
        _ => {
            let alerte = de == "alerte";
            let (fond, bord) = if alerte {
                (couleurs::ALERTE_FOND, couleurs::ALERTE_BORD)
            } else {
                (couleurs::SURFACE_2, couleurs::FILET)
            };
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                avatar(ui, alerte, opacite);
                ui.vertical(|ui| {
                    egui::Frame::new()
                        .fill(pale(fond))
                        .stroke((1.0, pale(bord)))
                        .corner_radius(CornerRadius { nw: 4, ne: 14, sw: 14, se: 14 })
                        .inner_margin(marge)
                        .show(ui, |ui| {
                            ui.set_max_width(max);
                            // Une bulle qui porte une carte prend toute sa largeur : la carte s'y étale.
                            if texte.contains("```") {
                                ui.set_width(max);
                            }
                            texte_d_eve(ui, texte, opacite, en_cours, cartes, gestes);
                            pieces_jointes(ui, pieces, opacite);
                        });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if !heure.is_empty() {
                            ui.label(RichText::new(heure).size(10.5).color(pale(couleurs::ENCRE_3)));
                        }
                        ce_qu_a_pris(ui, tour, pale(couleurs::ENCRE_3), FontId::proportional(10.5));
                        if !en_cours {
                            let police = FontId::proportional(10.5);
                            bouton_copier(ui, texte, pale(couleurs::ENCRE_3), couleurs::ACCENT, police);
                        }
                    });
                    ui.add_space(8.0);
                });
            });
        }
    }
}

/// Sous une réponse de Claude : ce que son tour a pris (« 12 s · 0,04 $ »), discret ; le cerveau qui l'a faite, au
/// survol. Rien sous une autre ligne (`tour` vide).
pub(crate) fn ce_qu_a_pris(ui: &mut egui::Ui, tour: &str, encre: Color32, police: FontId) {
    if tour.is_empty() {
        return;
    }
    let (court, cerveau) = tour.rsplit_once(" · ").unwrap_or((tour, ""));
    let bulle = format!("La durée de ce tour et son coût (au tarif de l'API) · {cerveau}");
    ui.add(egui::Label::new(RichText::new(court).font(police).color(encre)).selectable(false)).on_hover_text(bulle);
}

/// Le presse-papiers d'un bouton « Copier » (`rep`, identifié par `id`) : au clic, `texte()` y va. Rend vrai une
/// seconde et demie après, le temps de dire « Copié ».
pub(crate) fn copier_au_clic(
    ui: &egui::Ui,
    rep: &egui::Response,
    id: egui::Id,
    texte: impl FnOnce() -> String,
) -> bool {
    let maintenant = ui.input(|i| i.time);
    if rep.clicked() {
        ui.ctx().copy_text(texte());
        ui.ctx().data_mut(|d| d.insert_temp(id, maintenant));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1600));
    }
    ui.ctx().data(|d| d.get_temp::<f64>(id)).is_some_and(|t| maintenant - t < 1.5)
}

/// « Copier », sous une réponse d'Eve : son icône et son mot, discrets (`encre`), de la couleur d'`accent` au survol ;
/// au clic, la réponse telle qu'elle se lit ([`texte_a_copier`]) dans le presse-papiers, et « Copié » un moment. Les
/// trois styles du fil s'en servent.
pub(crate) fn bouton_copier(ui: &mut egui::Ui, reponse: &str, encre: Color32, accent: Color32, police: FontId) {
    let id = egui::Id::unique(("copier", reponse));
    let p = ui.painter();
    let icone = FontId::new(police.size, FontFamily::Name(ICONES.into()));
    // La place de « Copié » et de « Copier » : la plus large des deux, pour que rien ne bouge au clic.
    let g_mot = p.layout_no_wrap("Copier".into(), police.clone(), Color32::PLACEHOLDER);
    let g_icone = p.layout_no_wrap(icones::COPIER.to_string(), icone.clone(), Color32::PLACEHOLDER);
    let taille = vec2(g_icone.size().x + 4.0 + g_mot.size().x, g_mot.size().y.max(g_icone.size().y));
    let (r, rep) = ui.allocate_exact_size(taille, Sense::click());
    let copie = copier_au_clic(ui, &rep, id, || texte_a_copier(reponse));
    if ui.is_rect_visible(r) {
        let couleur = if copie || rep.hovered() { accent } else { encre };
        let (code, mot) = if copie { (icones::OUI, "Copié") } else { (icones::COPIER, "Copier") };
        let p = ui.painter();
        let g_icone = p.layout_no_wrap(code.to_string(), icone, couleur);
        let l_icone = g_icone.size().x;
        p.galley(egui::pos2(r.left(), r.center().y - g_icone.size().y / 2.0), g_icone, couleur);
        let g_mot = p.layout_no_wrap(mot.into(), police, couleur);
        p.galley(egui::pos2(r.left() + l_icone + 4.0, r.center().y - g_mot.size().y / 2.0), g_mot, couleur);
    }
    rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Copier cette réponse");
}

/// Une réponse d'Eve pour le presse-papiers, telle qu'elle se lit : sans son markdown (le gras, le code, les titres),
/// ses puces gardées, l'adresse d'un lien après son texte ; ses cartes en texte ([`cartes::en_texte`] : un tableau en
/// colonnes séparées par des tabulations, qui se colle dans Excel cellule par cellule), un dessin par son titre, un
/// bloc de code tel quel.
pub(crate) fn texte_a_copier(texte: &str) -> String {
    let mut morceaux: Vec<String> = Vec::new();
    for bloc in decouper(texte, false) {
        match bloc {
            Bloc::Texte(lignes) => {
                let lignes: Vec<String> = lignes
                    .iter()
                    .map(|l| {
                        if l.trim() == "---" {
                            return String::new();
                        }
                        let (mut job, mut liens) = (LayoutJob::default(), Liens::new());
                        ligne_en_forme(&mut job, &mut liens, l, false, 1.0, &theme::SOMBRES);
                        avec_adresses(&job.text, &liens)
                    })
                    .collect();
                let t = lignes.join("\n");
                if !t.trim().is_empty() {
                    morceaux.push(t.trim_matches('\n').to_string());
                }
            }
            Bloc::Code(code) => morceaux.push(code),
            Bloc::Carte(json) => morceaux.push(crate::cartes::en_texte(&json).unwrap_or(json)),
            Bloc::Dessin { svg, .. } => morceaux.push(format!("[Dessin : {}]", crate::cartes::titre_du_dessin(&svg))),
        }
    }
    morceaux.join("\n\n")
}

/// Le texte d'une ligne mise en forme, chaque lien suivi de son adresse entre parenthèses — sauf s'il l'écrit déjà.
fn avec_adresses(texte: &str, liens: &Liens) -> String {
    let lettres: Vec<char> = texte.chars().collect();
    let (mut sortie, mut i) = (String::new(), 0);
    for (place, adresse) in liens {
        sortie.extend(&lettres[i..place.end]);
        if lettres[place.clone()].iter().collect::<String>() != *adresse {
            sortie.push_str(&format!(" ({adresse})"));
        }
        i = place.end;
    }
    sortie.extend(&lettres[i..]);
    sortie
}

/// La petite orbe d'Eve, devant ses bulles ; une cloche pour une alerte.
pub(crate) fn avatar(ui: &mut egui::Ui, alerte: bool, opacite: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
    let (c, p) = (rect.center(), ui.painter());
    if alerte {
        p.circle_filled(c, 13.0, couleurs::ALERTE_FOND);
        p.circle_stroke(c, 12.5, (1.0, couleurs::ALERTE_BORD));
        p.text(
            c,
            Align2::CENTER_CENTER,
            icones::CLOCHE,
            FontId::new(12.0, FontFamily::Name(ICONES.into())),
            couleurs::AMBRE,
        );
    } else {
        sphere(p, c, 11.0);
        p.circle_stroke(c, 12.5, (1.0, Color32::from_white_alpha(40)));
    }
    if opacite < 1.0 {
        p.circle_filled(c, 13.5, couleurs::SURFACE.gamma_multiply(1.0 - opacite));
    }
}

fn heure_sous(ui: &mut egui::Ui, heure: &str, opacite: f32) {
    if !heure.is_empty() {
        ui.label(RichText::new(heure).size(10.5).color(couleurs::ENCRE_3.gamma_multiply(opacite)));
    }
    ui.add_space(8.0);
}

fn pieces_jointes(ui: &mut egui::Ui, pieces: &[&str], opacite: f32) {
    for p in pieces {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(icone(icones::PIECE, 11.0).color(couleurs::ENCRE_3.gamma_multiply(opacite)));
            ui.label(RichText::new(*p).size(11.5).color(couleurs::ENCRE_2.gamma_multiply(opacite)));
        });
    }
}

/// Eve réfléchit : trois points qui s'allument tour à tour, dans sa bulle.
fn points_de_suspension(ui: &mut egui::Ui) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        avatar(ui, false, 1.0);
        egui::Frame::new()
            .fill(couleurs::SURFACE_2)
            .stroke((1.0, couleurs::FILET))
            .corner_radius(CornerRadius { nw: 4, ne: 14, sw: 14, se: 14 })
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 8.0), Sense::hover());
                let t = ui.input(|i| i.time) as f32;
                for k in 0..3 {
                    let a = 0.3 + 0.7 * ((t * 5.0 - k as f32 * 0.8).sin() * 0.5 + 0.5);
                    let centre = egui::pos2(r.left() + 5.0 + k as f32 * 12.0, r.center().y);
                    ui.painter().circle_filled(centre, 3.5, couleurs::ENCRE_2.gamma_multiply(a));
                }
            });
    });
    ui.add_space(8.0);
}

/// Un bloc du texte d'Eve : du texte, un bloc de code, une carte (son JSON), ou un dessin (son SVG ; `fini` : sa
/// clôture est lue, ou la réponse est finie).
#[derive(Debug, PartialEq)]
enum Bloc {
    Texte(Vec<String>),
    Code(String),
    Carte(String),
    Dessin { svg: String, fini: bool },
}

/// Un texte qui est un SVG : il commence par sa balise, au besoin après la déclaration XML.
fn est_un_svg(t: &str) -> bool {
    let t = t.trim_start();
    t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg"))
}

/// Un tableau en markdown au début de `lignes` : son en-tête, sa ligne de tirets, ses rangées ; rendu en carte
/// « tableau » (son JSON) et le nombre de lignes qu'il prend. Claude en écrit souvent, au lieu d'une carte.
fn tableau_markdown(lignes: &[&str]) -> Option<(String, usize)> {
    let cellules = |l: &str| -> Option<Vec<String>> {
        let t = l.trim();
        let dedans = t.strip_prefix('|')?.strip_suffix('|')?;
        Some(dedans.split('|').map(|c| c.trim().to_string()).collect())
    };
    let tirets = |l: &str| {
        let t = l.trim();
        t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
    };
    let colonnes = cellules(lignes.first()?)?;
    if !lignes.get(1).is_some_and(|l| tirets(l)) {
        return None;
    }
    let rangees: Vec<Vec<String>> = lignes[2..].iter().map_while(|l| cellules(l)).collect();
    let json = serde_json::json!({ "type": "tableau", "titre": "", "colonnes": colonnes, "lignes": rangees });
    Some((json.to_string(), 2 + rangees.len()))
}

/// Le texte d'Eve en blocs, comme la page (`decouper` dans `eve.html`) : « ```carte » ouvre une carte (son JSON sur la
/// même ligne ou les suivantes, sa clôture au besoin collée à la fin), « ```svg » un dessin — un SVG dans un bloc
/// « ```xml », « ```html » ou sans nom en est un aussi —, un autre « ``` » un bloc de code, le reste est du texte.
/// Pendant l'écriture (`en_cours`), une clôture naissante en dernière ligne ne s'affiche pas, « ``` » ou « ```car » en
/// dernière ligne annonce déjà une carte, « ```s » un dessin.
fn decouper(texte: &str, en_cours: bool) -> Vec<Bloc> {
    let lignes: Vec<&str> = texte.lines().collect();
    let mut blocs = Vec::new();
    let mut tampon: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lignes.len() {
        let (l, derniere) = (lignes[i], i + 1 == lignes.len());
        let t = l.trim();
        // Un tableau en markdown (« | a | b | », puis « |---|---| ») devient une carte « tableau ».
        if let Some((json, n)) = tableau_markdown(&lignes[i..]) {
            if !tampon.is_empty() {
                blocs.push(Bloc::Texte(std::mem::take(&mut tampon)));
            }
            blocs.push(Bloc::Carte(json));
            i += n;
            continue;
        }
        let ouverture = t.strip_prefix("```").map(str::trim_start);
        // « ```carte {…} » : le JSON commence sur la ligne d'ouverture.
        let json_ici =
            ouverture.and_then(|o| o.strip_prefix("carte")).map(str::trim_start).filter(|j| j.starts_with('{'));
        let mot = ouverture.filter(|o| o.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-'));
        if json_ici.is_none() && mot.is_none() {
            // Pendant l'écriture, une clôture qui n'a encore qu'un ou deux accents graves ne s'affiche pas.
            if !(en_cours && derniere && !t.is_empty() && t.len() <= 2 && t.chars().all(|c| c == '`')) {
                tampon.push(l.to_string());
            }
            i += 1;
            continue;
        }
        if !tampon.is_empty() {
            blocs.push(Bloc::Texte(std::mem::take(&mut tampon)));
        }
        let carte =
            json_ici.is_some() || mot.is_some_and(|m| m == "carte" || (en_cours && derniere && "carte".starts_with(m)));
        let svg = !carte
            && mot.is_some_and(|m| m.eq_ignore_ascii_case("svg") || (en_cours && derniere && "svg".starts_with(m)));
        let mut contenu: Vec<&str> = Vec::new();
        if let Some(j) = json_ici {
            if let Some(seul) = j.trim_end().strip_suffix("```") {
                blocs.push(Bloc::Carte(seul.to_string()));
                i += 1;
                continue;
            }
            contenu.push(j);
        }
        i += 1;
        let mut ferme = false;
        while i < lignes.len() {
            let k = lignes[i];
            i += 1;
            if k.trim() == "```" {
                ferme = true;
                break;
            }
            // « …} ``` », « </svg>``` » : la clôture collée à la fin du JSON ou du dessin.
            if (carte || svg)
                && let Some(avant) = k.trim_end().strip_suffix("```")
            {
                contenu.push(avant);
                ferme = true;
                break;
            }
            contenu.push(k);
        }
        let contenu = contenu.join("\n");
        let dessin = svg
            || (!carte
                && mot.is_some_and(|m| matches!(m.to_ascii_lowercase().as_str(), "" | "xml" | "html"))
                && est_un_svg(&contenu));
        blocs.push(if carte {
            Bloc::Carte(contenu)
        } else if dessin {
            Bloc::Dessin { svg: contenu, fini: ferme || !en_cours }
        } else {
            Bloc::Code(contenu)
        });
    }
    if !tampon.is_empty() {
        blocs.push(Bloc::Texte(tampon));
    }
    blocs
}

/// Les cartes d'une réponse (leur JSON), découpées comme la fenêtre les dessine : le chef y prend les rappels que
/// Claude pose.
pub(crate) fn cartes_de(texte: &str) -> Vec<String> {
    decouper(texte, false)
        .into_iter()
        .filter_map(|b| if let Bloc::Carte(json) = b { Some(json) } else { None })
        .collect()
}

/// Le texte d'Eve mis en forme comme dans la page : **gras**, `code`, « --- » (ce qui suit est le détail écrit, plus
/// pâle), les blocs de code à chasse fixe, les cartes ([`Cartes`]) ; une puce pour « - ». Ses encres sont celles du
/// thème du `Ui` ([`theme::encres`]).
fn texte_d_eve(
    ui: &mut egui::Ui,
    texte: &str,
    opacite: f32,
    en_cours: bool,
    cartes: &mut Cartes,
    gestes: &mut Vec<Geste>,
) {
    let e = theme::encres(ui);
    let mut apres = false;
    for bloc in decouper(texte, en_cours) {
        match bloc {
            Bloc::Texte(lignes) => {
                // Les lignes vides des bords s'ôtent.
                let Some(a) = lignes.iter().position(|l| !l.trim().is_empty()) else { continue };
                let b = lignes.iter().rposition(|l| !l.trim().is_empty()).unwrap_or(a);
                let (mut job, mut liens) = (LayoutJob::default(), Liens::new());
                for l in &lignes[a..=b] {
                    if l.trim() == "---" {
                        if !job.text.is_empty() {
                            texte_avec_liens(ui, std::mem::take(&mut job), &std::mem::take(&mut liens), gestes);
                        }
                        filet(ui, e);
                        apres = true;
                        continue;
                    }
                    if !job.text.is_empty() {
                        job.append("\n", 0.0, format_de_base(apres, opacite, e));
                    }
                    ligne_en_forme(&mut job, &mut liens, &insecables(l), apres, opacite, e);
                }
                if !job.text.is_empty() {
                    texte_avec_liens(ui, job, &liens, gestes);
                }
            }
            Bloc::Code(code) => {
                egui::Frame::new()
                    .fill(e.bloc_fond)
                    .stroke((1.0, e.bloc_filet))
                    .corner_radius(8.0)
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(code).monospace().size(12.5).color(e.bloc_encre.gamma_multiply(opacite)),
                        );
                    });
            }
            Bloc::Carte(json) => {
                if let Some(g) = cartes.dessiner(ui, &json, en_cours, opacite) {
                    gestes.push(g);
                }
            }
            Bloc::Dessin { svg, fini } => {
                if let Some(g) = cartes.dessin(ui, &svg, fini, opacite) {
                    gestes.push(g);
                }
            }
        }
    }
}

fn format_de_base(apres: bool, opacite: f32, e: &theme::Encres) -> TextFormat {
    let (taille, encre) = if apres { (13.0, e.detail) } else { (14.0, e.texte) };
    TextFormat { font_id: FontId::proportional(taille), color: encre.gamma_multiply(opacite), ..Default::default() }
}

/// Les liens d'un texte d'Eve mis en page : la place de chacun (en caractères, comme les curseurs d'egui) et son
/// adresse — seulement `https://`, la seule que le chef ouvre.
type Liens = Vec<(std::ops::Range<usize>, String)>;

/// Un texte d'Eve et ses liens : au survol d'un lien, la main et son adresse (un texte de lien peut tromper, l'adresse
/// non) ; au clic, le geste `ouvrir_lien`, que le chef ouvre dans le navigateur. Le texte reste sélectionnable.
fn texte_avec_liens(ui: &mut egui::Ui, mut job: LayoutJob, liens: &Liens, gestes: &mut Vec<Geste>) {
    if liens.is_empty() {
        ui.label(job);
        return;
    }
    job.wrap.max_width = ui.available_width();
    let galee = ui.fonts_mut(|f| f.layout_job(job));
    // Le clic seul, sans le focus : un texte qui a le focus se souligne tout entier.
    let rep = ui.add(egui::Label::new(galee.clone()).sense(Sense::CLICK));
    let sous = |pos: egui::Pos2| {
        let i = caractere_sous(&galee, pos - rep.rect.min)?;
        liens.iter().find(|(place, _)| place.contains(&i)).map(|(_, adresse)| adresse.clone())
    };
    if let Some(adresse) = rep.hover_pos().and_then(sous) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if rep.clicked() {
            gestes.push(Geste { action: "ouvrir_lien", valeur: Value::String(adresse) });
        } else {
            rep.on_hover_text_at_pointer(adresse);
        }
    }
}

/// Le caractère d'un texte mis en page sous le point `p` (relatif au texte), compté en caractères comme les curseurs
/// d'egui (un saut de ligne compte pour un) ; rien entre deux lignes ni au-delà du bout d'une ligne.
fn caractere_sous(galee: &egui::Galley, p: egui::Vec2) -> Option<usize> {
    let mut debut = 0;
    for rangee in &galee.rows {
        if (rangee.min_y()..rangee.max_y()).contains(&p.y) {
            let x = p.x - rangee.pos.x;
            return rangee.row.glyphs.iter().position(|g| g.pos.x <= x && x < g.max_x()).map(|k| debut + k);
        }
        debut += rangee.char_count_including_newline().0;
    }
    None
}

/// Un morceau de texte ordinaire d'une ligne d'Eve : ses adresses nues (« https://… ») deviennent des liens. La
/// ponctuation qui suit une adresse (« . », « , », « » », une parenthèse fermante sans son ouvrante) n'en est pas.
fn ajouter_texte(job: &mut LayoutJob, liens: &mut Liens, texte: &str, format: &TextFormat, lien: &TextFormat) {
    let mut reste = texte;
    while let Some(debut) = reste.find("https://") {
        ajouter(job, &reste[..debut], format);
        let r = &reste[debut..];
        let mut adresse = &r[..r.find(char::is_whitespace).unwrap_or(r.len())];
        loop {
            let ouvrantes = adresse.matches('(').count();
            let fin = adresse.chars().last();
            if fin.is_some_and(|c| ".,;:!?»\"'".contains(c))
                || (fin == Some(')') && adresse.matches(')').count() > ouvrantes)
            {
                adresse = &adresse[..adresse.len() - fin.map_or(0, char::len_utf8)];
            } else {
                break;
            }
        }
        if adresse.len() > "https://".len() {
            lier(job, liens, adresse, adresse, lien);
        } else {
            ajouter(job, adresse, format);
        }
        reste = &r[adresse.len()..];
    }
    ajouter(job, reste, format);
}

/// Un lien ajouté à `job` : son texte au format des liens, et sa place retenue s'il s'ouvre (`https://`).
fn lier(job: &mut LayoutJob, liens: &mut Liens, texte: &str, adresse: &str, format: &TextFormat) {
    let debut = job.text.chars().count();
    ajouter(job, texte, format);
    if adresse.starts_with("https://") && !adresse.chars().any(char::is_control) {
        liens.push((debut..job.text.chars().count(), adresse.to_string()));
    }
}

/// Une ligne de texte d'Eve, ajoutée à `job` : ses **gras** en demi-gras, ses `code` à chasse fixe sur un fond, ses
/// liens « [texte](adresse) » par leur seul texte, soulignés, et ses adresses nues ([`ajouter_texte`]) — leur place va
/// dans `liens` ; « - » ou « * » en tête devient une puce, « # » un titre. Un repère sans sa fin reste tel quel.
fn ligne_en_forme(job: &mut LayoutJob, liens: &mut Liens, ligne: &str, apres: bool, opacite: f32, e: &theme::Encres) {
    let normal = format_de_base(apres, opacite, e);
    let taille = normal.font_id.size;
    let gras = TextFormat { font_id: FontId::new(taille, FontFamily::Name(SEMI.into())), ..normal.clone() };
    let titre =
        TextFormat { color: if apres { normal.color } else { e.titre.gamma_multiply(opacite) }, ..gras.clone() };
    let code = TextFormat {
        font_id: FontId::monospace(taille - 1.5),
        color: e.code_encre.gamma_multiply(opacite),
        background: e.code_fond,
        ..normal.clone()
    };
    let lien = TextFormat {
        color: e.accent.gamma_multiply(opacite),
        underline: Stroke::new(1.0, e.accent.gamma_multiply(0.45 * opacite)),
        ..normal.clone()
    };
    let l = ligne.trim_end();
    let t = l.trim_start();
    let retrait = (l.len() - t.len()).min(8);
    if retrait > 0 {
        ajouter(job, &" ".repeat(retrait), &normal);
    }
    let titre_de_section = t.strip_prefix('#').map(|r| r.trim_start_matches('#')).and_then(|r| r.strip_prefix(' '));
    let puce = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "));
    let (base, mut reste) = match (titre_de_section, puce) {
        (Some(r), _) => (&titre, r.trim_start()),
        (None, Some(r)) => {
            ajouter(job, "•  ", &TextFormat { color: e.puce.gamma_multiply(opacite), ..normal.clone() });
            (&normal, r)
        }
        _ => (&normal, t),
    };
    while !reste.is_empty() {
        // Le prochain repère : « ** », « ` » ou « [ ».
        let (g, c, l) = (reste.find("**"), reste.find('`'), reste.find('['));
        let Some(debut) = [g, c, l].into_iter().flatten().min() else {
            ajouter_texte(job, liens, reste, base, &lien);
            break;
        };
        ajouter_texte(job, liens, &reste[..debut], base, &lien);
        let r = &reste[debut..];
        let (marque, format) = if Some(debut) == g { ("**", &gras) } else { ("`", &code) };
        if Some(debut) != l {
            if let Some(fin) = r[marque.len()..].find(marque) {
                ajouter(job, &r[marque.len()..marque.len() + fin], format);
                reste = &r[marque.len() + fin + marque.len()..];
                continue;
            }
        } else if let Some(milieu) = r.find("](")
            && !r[1..milieu].contains(']')
            && let Some(fin) = r[milieu + 2..].find(')')
            && !r[milieu + 2..milieu + 2 + fin].contains(char::is_whitespace)
        {
            lier(job, liens, &r[1..milieu], &r[milieu + 2..milieu + 2 + fin], &lien);
            reste = &r[milieu + 2 + fin + 1..];
            continue;
        }
        let n = if Some(debut) == g { 2 } else { 1 };
        ajouter(job, &r[..n], base);
        reste = &r[n..];
    }
}

/// Une ligne à la française, pour l'écran seulement (le texte copié garde ses espaces) : une espace insécable devant
/// « ? ! : ; » et « », après « et entre un nombre et ce qui le suit (son unité, ses milliers, « h »). Toujours U+00A0 :
/// c'est la seule qu'egui ne coupe pas — il coupe à l'espace fine (U+202F), que Claude écrit parfois entre les
/// milliers. Une bulle ne se coupe plus au mauvais endroit (« soumission / ? », « 1 / 250 $ »).
pub(crate) fn insecables(texte: &str) -> String {
    let mut s = texte.replace('\u{202f}', "\u{a0}");
    for (de, a) in [
        (" ?", "\u{a0}?"),
        (" !", "\u{a0}!"),
        (" :", "\u{a0}:"),
        (" ;", "\u{a0};"),
        ("« ", "«\u{a0}"),
        (" »", "\u{a0}»"),
        (" $", "\u{a0}$"),
        (" %", "\u{a0}%"),
        (" M$", "\u{a0}M$"),
    ] {
        s = s.replace(de, a);
    }
    // Les heures (« 6 h 30 », « 17 h ») et les milliers (« 142 000 ») : un nombre ne se sépare pas de ce qui le suit.
    let c: Vec<char> = s.chars().collect();
    let a = |k: usize| c.get(k).copied().unwrap_or(' ');
    (0..c.len())
        .map(|k| match c[k] {
            ' ' if k > 0 && a(k - 1).is_ascii_digit() && a(k + 1).is_ascii_digit() => '\u{a0}',
            ' ' if k > 0 && a(k - 1).is_ascii_digit() && a(k + 1) == 'h' && !a(k + 2).is_alphabetic() => '\u{a0}',
            ' ' if k > 2
                && a(k - 1) == 'h'
                && a(k - 2) == ' '
                && a(k - 3).is_ascii_digit()
                && a(k + 1).is_ascii_digit() =>
            {
                '\u{a0}'
            }
            x => x,
        })
        .collect()
}

/// `job.append`, sans les morceaux vides.
fn ajouter(job: &mut LayoutJob, texte: &str, format: &TextFormat) {
    if !texte.is_empty() {
        job.append(texte, 0.0, format.clone());
    }
}

/// Le séparateur « --- » d'une réponse.
fn filet(ui: &mut egui::Ui, e: &theme::Encres) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 11.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, e.filet));
}

// ---- Les actions --------------------------------------------------------------------------------------------------

/// Une ligne du fil, comme une entrée du flux du tableau : un liseré à gauche (cyan pour la personne, vert pour Eve,
/// rouge pour une alerte), l'heure et qui parle en petites capitales, puis le texte ; Eve-même, au centre, en gris.
fn ligne_du_flux(ui: &mut egui::Ui, l: &Ligne<'_>, cartes: &mut Cartes, gestes: &mut Vec<Geste>) {
    let Ligne { de, texte, heure, ancien, pieces, en_cours, tour } = *l;
    let opacite = if ancien { 0.55 } else { 1.0 };
    if de == "systeme" {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(texte).monospace().size(11.0).color(hud::GRIS.gamma_multiply(opacite)));
        });
        ui.add_space(6.0);
        return;
    }
    let (couleur, qui, fond) = match de {
        "toi" => (hud::CYAN, "▶ TOI", hud::CYAN.gamma_multiply(0.05)),
        "alerte" => (hud::ROUGE_PALE, "◆ ALERTE", hud::ROUGE.gamma_multiply(0.09)),
        _ => (hud::VERT, "◀ EVE", hud::VERT.gamma_multiply(0.05)),
    };
    let r = egui::Frame::new()
        .fill(fond.gamma_multiply(opacite))
        .inner_margin(Margin { left: 11, right: 8, top: 5, bottom: 7 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if !heure.is_empty() {
                    ui.label(RichText::new(heure).monospace().size(11.0).color(hud::GRIS.gamma_multiply(opacite)));
                }
                ui.label(centre::capitales(qui, 11.0, couleur.gamma_multiply(opacite)));
                ce_qu_a_pris(ui, tour, hud::GRIS.gamma_multiply(opacite), FontId::monospace(10.5));
                if en_cours {
                    ui.label(centre::capitales("· EN COURS", 11.0, hud::GRIS));
                } else if de != "toi" {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let police = FontId::monospace(10.5);
                        bouton_copier(ui, texte, hud::GRIS.gamma_multiply(opacite), hud::CYAN, police);
                    });
                }
            });
            if de == "toi" {
                ui.label(RichText::new(texte).size(14.0).color(hud::TEXTE.gamma_multiply(opacite)));
            } else {
                texte_d_eve(ui, texte, opacite, en_cours, cartes, gestes);
            }
            pieces_jointes(ui, pieces, opacite);
        })
        .response
        .rect;
    let lisere = egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 2.0, r.bottom()));
    ui.painter().rect_filled(lisere, 0.0, couleur.gamma_multiply(opacite));
    ui.add_space(7.0);
}

/// Un texte du tableau, à chasse fixe et espacé, posé à `pos` selon `ancre` ; rend sa place.
fn texte_hud(
    p: &egui::Painter,
    pos: egui::Pos2,
    ancre: Align2,
    texte: &str,
    taille: f32,
    couleur: Color32,
    espace: f32,
) -> egui::Rect {
    let mut job = LayoutJob::default();
    let format = TextFormat {
        font_id: FontId::monospace(taille),
        color: couleur,
        extra_letter_spacing: espace,
        ..Default::default()
    };
    job.append(texte, 0.0, format);
    let galley = p.layout_job(job);
    let r = ancre.anchor_size(pos, galley.size());
    p.galley(r.min, galley, couleur);
    r
}

/// La ligne d'état du tableau (cdc:19-20), centrée sous `pos` : le texte tapé, puis le curseur, un pavé cyan de 9 sur
/// 15 qui clignote. Éteint (une espace au bout de la ligne), il garde sa place : la ligne ne bouge pas.
fn ligne_d_etat_hud(p: &egui::Painter, pos: egui::Pos2, ligne: &str) -> egui::Rect {
    let (corps, allume) = match ligne.strip_suffix(CURSEUR) {
        Some(corps) => (corps, true),
        None => (ligne.strip_suffix(' ').unwrap_or(ligne), false),
    };
    let mut job = LayoutJob::default();
    let format = TextFormat {
        font_id: FontId::monospace(15.0),
        color: hud::BLANC,
        extra_letter_spacing: 2.1,
        ..Default::default()
    };
    job.append(corps, 0.0, format);
    let galley = p.layout_job(job);
    let texte = galley.size();
    let r = Align2::CENTER_TOP.anchor_size(pos, vec2(texte.x + 3.0 + 9.0, texte.y.max(15.0)));
    p.galley(r.min, galley, hud::BLANC);
    if allume {
        let pave = egui::Rect::from_min_size(egui::pos2(r.left() + texte.x + 3.0, r.center().y - 6.5), vec2(9.0, 15.0));
        p.rect_filled(pave, 0.0, hud::CYAN);
    }
    r
}

/// La prochaine échéance, pour l'écran de veille (cdc:581) : « PROCHAINE ÉCHÉANCE · DANS 7 J · … », ou rien.
fn prochaine_echeance(echeances: &Value, aujourdhui: &str) -> String {
    echeances
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let jours = centre::jours_entre(aujourdhui, e["d"].as_str()?)?;
            (jours >= 0).then(|| (jours, e["q"].as_str().unwrap_or("").to_string()))
        })
        .min_by_key(|(jours, _)| *jours)
        .map(|(jours, texte)| ambiance::prochaine_echeance(jours, &texte))
        .unwrap_or_default()
}

/// Ce que fait Eve, dit comme la page dit ses actions, pour la ligne d'état quand aucun outil ne court.
fn phrase_de_repos(etat: &str) -> &'static str {
    match etat {
        "ecoute" => "EVE T'ÉCOUTE",
        "transcription" => "EVE TRANSCRIT",
        "confirmation" => "EVE ATTEND TON ACCORD",
        "reflexion" => "EVE S'EN OCCUPE",
        "parole" => "EVE TE RÉPOND",
        "demarrage" => "EVE SE PRÉPARE",
        "erreur" => "EVE A UN PROBLÈME",
        _ => "EVE À L'ÉCOUTE",
    }
}

/// L'état de la liaison, à la place où la page le dit (cdc:93, 519, 591, 773, 823) : le mode du réseau, s'il en a un.
fn etat_de_la_liaison(modes: crate::reseau::Modes) -> &'static str {
    if modes.demo {
        "MODE DÉMO · touche D pour arrêter"
    } else if modes.rediffusion {
        "REDIFFUSION DE LA JOURNÉE · J POUR ARRÊTER"
    } else if modes.econome_de_lui_meme {
        "MODE ÉCONOME : LA FENÊTRE S'EST ALLÉGÉE · E POUR BASCULER"
    } else if modes.econome {
        "MODE ÉCONOME ACTIF · E POUR BASCULER"
    } else {
        "LIAISON ACTIVE · Eve en direct"
    }
}

/// L'horloge du tableau, « 20 h 29 min 09 s · 2026-10-23 », d'après l'horodatage du journal (« 2026-10-23 20-29-09 »).
fn horloge(horodatage: &str) -> String {
    match (horodatage.get(..10), horodatage.get(11..13), horodatage.get(14..16), horodatage.get(17..19)) {
        (Some(d), Some(h), Some(m), Some(s)) => {
            let h = h.strip_prefix('0').filter(|x| !x.is_empty()).unwrap_or(h);
            format!("{h} h {m} min {s} s · {d}")
        }
        _ => horodatage.to_string(),
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_texte_d_eve_se_decoupe_comme_dans_la_page() {
        let texte = |l: &[&str]| Bloc::Texte(l.iter().map(|s| s.to_string()).collect());
        let carte = |j: &str| Bloc::Carte(j.to_string());
        assert_eq!(
            decouper("Voici le budget.\n---\n```carte\n{\"type\":\"tableau\"}\n```\nFin.", false),
            [texte(&["Voici le budget.", "---"]), carte("{\"type\":\"tableau\"}"), texte(&["Fin."])]
        );
        assert_eq!(
            decouper("Le script :\n```python\nprint(1)\n```", false),
            [texte(&["Le script :"]), Bloc::Code("print(1)".into())],
            "un bloc de code reste du code"
        );
        // Une carte d'une ligne, une clôture collée au JSON : la suite du texte reste.
        let attendu = [texte(&["A"]), carte("{\"type\":\"agenda\"}"), texte(&["B"])];
        assert_eq!(decouper("A\n```carte {\"type\":\"agenda\"}```\nB", false), attendu);
        assert_eq!(decouper("A\n```carte\n{\"type\":\"agenda\"}```\nB", false), attendu);
        // Pendant l'écriture : une clôture naissante ne s'affiche pas, « ```car » annonce déjà une carte.
        assert_eq!(decouper("A\n``", true), [texte(&["A"])]);
        assert_eq!(decouper("A\n```car", true), [texte(&["A"]), carte("")]);
        assert_eq!(
            decouper("Trois accents ``` au milieu d'une phrase.", false),
            [texte(&["Trois accents ``` au milieu d'une phrase."])]
        );
        // Un tableau en markdown devient une carte « tableau » ; une ligne de barres seule reste du texte.
        let blocs =
            decouper("Voici :\n| Date | Quoi |\n|---|:---:|\n| 31 oct. | Halloween |\n| 1 nov. | Heure |\nFin.", false);
        assert_eq!(blocs.len(), 3);
        let Bloc::Carte(json) = &blocs[1] else { panic!("{blocs:?}") };
        let v: Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["type"], "tableau");
        assert_eq!(v["colonnes"], serde_json::json!(["Date", "Quoi"]));
        assert_eq!(v["lignes"][1], serde_json::json!(["1 nov.", "Heure"]));
        assert_eq!(blocs[2], texte(&["Fin."]));
        assert_eq!(decouper("| a | b |\nsuite", false), [texte(&["| a | b |", "suite"])]);
    }

    #[test]
    fn les_themes_tiennent_dans_leurs_segments() {
        // Le panneau des réglages : 380 points, moins ses marges (14 de chaque côté), celles du cadre des segments (4)
        // et leurs écarts (3) ; chaque mot, centré, garde 4 points de chaque côté.
        let ctx = egui::Context::default();
        polices(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let n = crate::reglages::THEMES.len() as f32;
            let segment = (380.0 - 28.0 - 8.0 - 3.0 * (n - 1.0)) / n;
            for (_, mot) in crate::reglages::THEMES {
                let police = FontId::new(13.5, FontFamily::Name(SEMI.into()));
                let l = ui.painter().layout_no_wrap(mot.to_string(), police, Color32::WHITE).size().x;
                assert!(l + 8.0 <= segment, "« {mot} » : {l} points dans un segment de {segment}");
            }
        });
    }

    #[test]
    fn un_bloc_svg_est_un_dessin() {
        let texte = |l: &[&str]| Bloc::Texte(l.iter().map(|s| s.to_string()).collect());
        let dessin = |svg: &str, fini: bool| Bloc::Dessin { svg: svg.to_string(), fini };
        let svg = "<svg viewBox=\"0 0 10 10\">\n<rect width=\"10\" height=\"10\"/>\n</svg>";
        assert_eq!(
            decouper(&format!("Voici le plan.\n---\n```svg\n{svg}\n```\nFin."), false),
            [texte(&["Voici le plan.", "---"]), dessin(svg, true), texte(&["Fin."])]
        );
        // Un SVG dans un bloc « ```xml », « ```html » ou sans nom est un dessin aussi ; une autre page reste du code.
        for nom in ["xml", "html", "", "SVG"] {
            assert_eq!(decouper(&format!("```{nom}\n{svg}\n```"), false), [dessin(svg, true)], "« {nom} »");
        }
        let declare = format!("<?xml version=\"1.0\"?>\n{svg}");
        assert_eq!(decouper(&format!("```xml\n{declare}\n```"), false), [dessin(&declare, true)]);
        assert_eq!(decouper("```html\n<p>Bonjour</p>\n```", false), [Bloc::Code("<p>Bonjour</p>".into())]);
        // La clôture collée à la fin du dessin.
        assert_eq!(decouper("```svg\n<svg></svg>```\nB", false), [dessin("<svg></svg>", true), texte(&["B"])]);
        // Pendant l'écriture : pas fini tant que sa clôture n'est pas lue ; « ```s » l'annonce déjà ; la réponse finie,
        // un dessin sans clôture se dessine quand même.
        assert_eq!(decouper("```svg\n<svg viewBox", true), [dessin("<svg viewBox", false)]);
        assert_eq!(decouper("A\n```sv", true), [texte(&["A"]), dessin("", false)]);
        assert_eq!(decouper(&format!("```svg\n{svg}"), false), [dessin(svg, true)]);
        assert_eq!(decouper("A\n```car", true), [texte(&["A"]), Bloc::Carte(String::new())], "« ```car » : une carte");
    }

    /// Une ligne mise en forme, et ses liens.
    fn en_forme(ligne: &str) -> (LayoutJob, Liens) {
        let (mut job, mut liens) = (LayoutJob::default(), Liens::new());
        ligne_en_forme(&mut job, &mut liens, ligne, false, 1.0, &theme::SOMBRES);
        (job, liens)
    }

    #[test]
    fn un_texte_a_la_francaise() {
        assert_eq!(
            insecables("la soumission ? Oui : 1,45 M$, 60 %, à 6 h 30, à 17 h. Environ 142 000 $ !"),
            "la soumission\u{a0}? Oui\u{a0}: 1,45\u{a0}M$, 60\u{a0}%, à 6\u{a0}h\u{a0}30, à 17\u{a0}h. Environ \
             142\u{a0}000\u{a0}$\u{a0}!"
        );
        assert_eq!(insecables("« le Mur », à 16 h à l'entrepôt"), "«\u{a0}le Mur\u{a0}», à 16\u{a0}h à l'entrepôt");
        // L'espace fine que Claude écrit entre les milliers, egui la coupe : elle devient insécable.
        assert_eq!(insecables("1\u{202f}250 $"), "1\u{a0}250\u{a0}$");
        // Le reste ne bouge pas : une liste, un lien, du code.
        for l in ["- Lundi et mardi", "[le site](https://exemple.ca/a?b=1)", "`a?b`", "Bonjour Sam."] {
            assert_eq!(insecables(l), l);
        }
    }

    /// Ce qu'egui fait d'une ligne trop longue : le « ? » et le montant ne partent jamais seuls à la ligne suivante.
    #[test]
    fn une_bulle_ne_se_coupe_pas_au_mauvais_endroit() {
        let ctx = egui::Context::default();
        polices(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter();
            // Des largeurs où chaque mot tient (plus étroite, egui coupe un mot là où il peut).
            for largeur in (120..400).step_by(7).map(|l| l as f32) {
                for texte in ["Tu veux que je relance la soumission ?", "La facture est de 1 250 $ en tout."] {
                    let mut job = LayoutJob::single_section(insecables(texte), TextFormat::default());
                    job.wrap.max_width = largeur;
                    for r in &p.layout_job(job).rows {
                        let r = r.text();
                        let r = r.trim();
                        assert!(!r.starts_with(['?', '$']) && !r.starts_with("250"), "{texte:?} à {largeur} : {r:?}");
                    }
                }
            }
        });
    }

    #[test]
    fn une_reponse_se_copie_comme_elle_se_lit() {
        let reponse = "Voici le **budget** de la sortie :\n\n| Poste | Montant |\n|---|---:|\n| Autobus | 120,00 $ |\n| \
                       Collation | 36,00 $ |\n\nEn tout : **156,00 $**, voir [la grille](https://exemple.ca/grille).\n\
                       ---\nLu dans `budget.md`.";
        assert_eq!(
            texte_a_copier(reponse),
            "Voici le budget de la sortie :\n\nPoste\tMontant\nAutobus\t120,00 $\nCollation\t36,00 $\n\nEn tout : \
             156,00 $, voir la grille (https://exemple.ca/grille).\n\nLu dans budget.md."
        );
        // Les puces restent, un dessin se dit par son titre, une adresse nue ne se répète pas.
        let reponse = "Le plan :\n- une https://exemple.ca\n```svg\n<svg><title>Plan de la maison</title></svg>\n```";
        assert_eq!(texte_a_copier(reponse), "Le plan :\n•  une https://exemple.ca\n\n[Dessin : Plan de la maison]");
    }

    #[test]
    fn les_liens_s_ouvrent_a_leur_place() {
        // Un lien en markdown : son texte seul se voit, sa place va à son adresse.
        let (job, liens) = en_forme("Le formulaire : [TP-1](https://www.revenuquebec.ca/tp1/) ici.");
        assert_eq!(job.text, "Le formulaire : TP-1 ici.");
        assert_eq!(liens, [(16..20, "https://www.revenuquebec.ca/tp1/".to_string())]);
        // Une adresse nue : la ponctuation qui la suit n'en est pas, ni la parenthèse qui l'entoure ; une parenthèse
        // qui lui appartient reste.
        let (job, liens) =
            en_forme("Voir https://exemple.ca/a. Et (https://exemple.ca/b), puis https://fr.wikipedia.org/wiki/X_(y).");
        assert_eq!(
            job.text,
            "Voir https://exemple.ca/a. Et (https://exemple.ca/b), puis https://fr.wikipedia.org/wiki/X_(y)."
        );
        let adresses: Vec<&str> = liens.iter().map(|(_, a)| a.as_str()).collect();
        assert_eq!(adresses, ["https://exemple.ca/a", "https://exemple.ca/b", "https://fr.wikipedia.org/wiki/X_(y)"]);
        let texte: Vec<char> = job.text.chars().collect();
        for (place, adresse) in &liens {
            assert_eq!(texte[place.clone()].iter().collect::<String>(), *adresse, "la place est celle du texte");
        }
        // Les places comptent des caractères, pas des octets : un accent avant le lien ne les décale pas.
        let (_, liens) = en_forme("Échéancier : https://exemple.ca");
        assert_eq!(liens, [(13..31, "https://exemple.ca".to_string())]);
        // Ni « http:// », ni « javascript: », ni « file: » ne s'ouvrent : leur texte se voit, sans lien.
        let (job, liens) = en_forme("[a](http://exemple.ca) [b](javascript:alert) [c](file:///C:/x) https://");
        assert_eq!(job.text, "a b c https://");
        assert!(liens.is_empty(), "{liens:?}");
        // Dans un `code`, une adresse reste du code.
        let (_, liens) = en_forme("`https://exemple.ca`");
        assert!(liens.is_empty());
    }

    #[test]
    fn le_gras_et_le_code_se_mettent_en_forme() {
        let (job, _) = en_forme("Le fichier `questions.js` contient **7 questions**.");
        assert_eq!(job.text, "Le fichier questions.js contient 7 questions.");
        let familles: Vec<FontFamily> = job.sections.iter().map(|s| s.format.font_id.family.clone()).collect();
        assert_eq!(
            familles,
            [
                FontFamily::Proportional,
                FontFamily::Monospace,
                FontFamily::Proportional,
                FontFamily::Name(SEMI.into()),
                FontFamily::Proportional
            ]
        );
        let (job, _) = en_forme("- une **puce** inachevée `ici");
        assert_eq!(job.text, "•  une puce inachevée `ici", "un accent grave seul reste tel quel");
        let (job, _) =
            en_forme("Voir ([publicholidays.com](https://publicholidays.com/halloween/)) et [ceci] tel quel.");
        assert_eq!(job.text, "Voir (publicholidays.com) et [ceci] tel quel.", "un lien se dit par son texte");
        assert_eq!(job.sections.len(), 3);
        assert_eq!(job.sections[1].format.color, couleurs::ACCENT);
        let (job, _) = en_forme("## Le budget");
        assert_eq!(job.text, "Le budget");
        assert_eq!(job.sections[0].format.font_id.family, FontFamily::Name(SEMI.into()));
    }

    #[test]
    fn le_degrade_de_l_orbe_va_du_reflet_au_bord() {
        assert_eq!(degrade_orbe(0.0), couleurs::ORBE[0]);
        assert_eq!(degrade_orbe(1.0), couleurs::ORBE[3]);
        assert_eq!(melange(Color32::BLACK, Color32::WHITE, 0.5), Color32::from_rgb(128, 128, 128));
    }

    #[test]
    fn un_reglage_change_ici_tient_quatre_secondes() {
        let etat = serde_json::json!({
            "reglages": { "sourdine": false, "alertes": { "factures": true } },
            "eveil": true,
        });
        let mut tenus = HashMap::new();
        assert_eq!(valeur_reglage(&etat, &tenus, "sourdine", 10.0), Some(false));
        assert_eq!(valeur_reglage(&etat, &tenus, "alertes.factures", 10.0), Some(true));
        assert_eq!(valeur_reglage(&etat, &tenus, "eveil", 10.0), Some(true), "à défaut, le mot d'éveil de l'état");
        assert_eq!(valeur_reglage(&etat, &tenus, "devant", 10.0), None, "sans valeur : grisé");
        tenus.insert("sourdine".to_string(), (true, 10.0));
        assert_eq!(valeur_reglage(&etat, &tenus, "sourdine", 13.9), Some(true), "tenue le temps que le chef publie");
        assert_eq!(valeur_reglage(&etat, &tenus, "sourdine", 14.1), Some(false));
    }

    #[test]
    fn la_veille_dit_la_prochaine_echeance() {
        let echeances = serde_json::json!([
            { "d": "2026-10-20", "q": "Passée" },
            { "d": "2026-11-01", "q": "Changement d'heure" },
            { "d": "2026-10-31", "q": "Halloween" },
        ]);
        assert!(prochaine_echeance(&echeances, "2026-10-24").contains("HALLOWEEN"));
        assert_eq!(prochaine_echeance(&serde_json::json!([]), "2026-10-24"), "");
    }

    #[test]
    fn la_ligne_d_etat_dit_ce_que_fait_eve() {
        assert_eq!(phrase_de_repos("pret"), "EVE À L'ÉCOUTE");
        assert_eq!(phrase_de_repos("reflexion"), "EVE S'EN OCCUPE");
        assert_eq!(phrase_de_repos("parole"), "EVE TE RÉPOND");
        assert_eq!(phrase_de_repos("inconnu"), "EVE À L'ÉCOUTE");
        let demo = crate::reseau::Modes { demo: true, econome: true, ..Default::default() };
        assert_eq!(etat_de_la_liaison(demo), "MODE DÉMO · touche D pour arrêter");
        assert_eq!(etat_de_la_liaison(Default::default()), "LIAISON ACTIVE · Eve en direct");
    }

    #[test]
    fn l_horloge_du_tableau() {
        assert_eq!(horloge("2026-10-23 20-29-09"), "20 h 29 min 09 s · 2026-10-23");
        assert_eq!(horloge("2026-10-24 09-05-00"), "9 h 05 min 00 s · 2026-10-24");
        assert_eq!(horloge("2026-10-24 00-00-07"), "0 h 00 min 07 s · 2026-10-24");
    }

    #[test]
    fn l_icone_de_la_fenetre_est_le_logo_sur_un_carre_arrondi() {
        let i = icone_de_fenetre().expect("le logo se lit");
        assert_eq!((i.width, i.height, i.rgba.len()), (64, 64, 64 * 64 * 4));
        let px = |x: usize, y: usize| &i.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(px(0, 0)[3], 0, "le coin est arrondi, donc transparent");
        assert_eq!(px(0, 32)[3], 255, "le bord est plein");
    }
}
