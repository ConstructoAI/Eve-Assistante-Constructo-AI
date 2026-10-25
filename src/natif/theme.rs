//! Le thème « Constructo AI » de la fenêtre native : clair, simple et professionnel, habillé comme l'ERP de Constructo
//! AI (`ERP_REACT\frontend`) — l'autre habit de la fenêtre, à côté du centre de commande. Ce qu'il en prend :
//!
//! - le fond qui respire (`src\index.css`, `body` et `.erp-fond-app`, l. 118-131) : un dégradé à −45° du blanc à
//!   `--ui-bg-1` (#BFDBFE) et `--ui-bg-2` (#DBEAFE), quatre fois plus grand que la zone, qui va et vient en 20 s
//!   (`bgBreath`, sur la courbe `ease`) ; figé au milieu de sa course quand Windows ne veut pas d'animations, comme
//!   celui de l'ERP sous `prefers-reduced-motion` (l. 169-177) ;
//! - ses cartes (`.erp-card`, l. 336-340 ; la carte d'indicateur et la carte de section d'`AnalyticsPage.tsx`,
//!   l. 201-265), à la manière des tableaux de bord d'aujourd'hui : blanches, un rayon de 14 points, un filet froid,
//!   deux ombres douces teintées de marine, leur haut lavé de leur teinte, leur icône dans une pastille en dégradé vif,
//!   leur titre en demi-gras ; ses badges pastel (`Badge.tsx`) ; la météo de son badge du jour
//!   (`DashboardWeatherBadge.tsx`) ;
//! - la conversation comme le dock de son assistant (`AssistantDock.tsx`, `MessageBubble.tsx`, `AssistantIAChat.tsx`) :
//!   une colonne à droite que le fond traverse, son en-tête marine (`.erp-topbar`, l. 417-420), les bulles d'Eve
//!   blanches à peine bleutées (`.ai-bubble-assistant`, l. 905-908), celles de la personne en dégradé bleu, la saisie
//!   `.erp-input` (l. 383-392) et le bouton primaire #0078D4 ; le texte des réponses comme son markdown
//!   (`utils\aiMarkdown.ts`) ;
//! - sa palette (`tailwind.config.js`, l. 36-129 ; `index.css`, l. 235-274 et les gris de Fluent, l. 606-670).
//!
//! Pas de réseau neuronal. Le centre déplié, le tableau de bord à gauche, une page qui défile — la bannière d'accueil
//! (un dégradé du marine de Constructo AI au bleu du ciel : l'orbe d'Eve, le salut, la date, ce que fait Eve, la
//! météo), la décision du jour, les échéances en pages de calendrier et tout le panneau d'information en cartes
//! ([`crate::centre`]) — et la conversation à droite ; replié (ou la fenêtre étroite), la conversation seule, son orbe en
//! tête. Un thème de bureau, calme comme l'ERP : ni musique ni notes, ni la nuit, les saisons ou l'écran de veille du
//! tableau, ni ses touches. La barre marine reste celle de la fenêtre ([`super::Fenetre`]), comme celle de l'ERP.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Mesh, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, UiBuilder, pos2, vec2,
};
use eve::parole::Emotion;
use serde_json::Value;

use super::{
    Fenetre, GRAS, ICONES, Ligne, SEMI, Style, TENUE, couleurs, degrade, icones, melange, segments, semi, sphere,
};
use super::{pieces, portrait};
use crate::cartes::{Cartes, Geste};
use crate::reglages::THEMES;
use crate::{ambiance, journal};

/// La palette de l'ERP de Constructo AI : `index.css` (ses variables, et les gris de Tailwind qu'il ramène à ceux de
/// Fluent), `tailwind.config.js` (ses bleus, ses teintes d'état), ses composants (les pastels des badges et des cartes
/// d'indicateurs).
pub(crate) mod erp {
    use eframe::egui::Color32;

    /// Le bleu de Constructo AI (`--ui-primary`) et son survol (`--ui-primary-dark`).
    pub const PRIMAIRE: Color32 = Color32::from_rgb(0x00, 0x78, 0xD4);
    pub const PRIMAIRE_FONCE: Color32 = Color32::from_rgb(0x00, 0x5E, 0xA2);
    /// Le marine de la barre et de l'en-tête du dock (`--ui-topbar-from`, `--ui-topbar-to`).
    pub const MARINE_HAUT: Color32 = Color32::from_rgb(0x00, 0x2B, 0x6B);
    pub const MARINE: Color32 = Color32::from_rgb(0x00, 0x20, 0x50);
    /// Le fond qui respire, entre des blancs : `--ui-bg-1`, `--ui-bg-2`.
    pub const FOND_1: Color32 = Color32::from_rgb(0xBF, 0xDB, 0xFE);
    pub const FOND_2: Color32 = Color32::from_rgb(0xDB, 0xEA, 0xFE);
    /// Les encres de Fluent : le texte (`text-gray-900`), le secondaire (`-600`), le discret (`-400`).
    pub const TEXTE: Color32 = Color32::from_rgb(0x32, 0x31, 0x30);
    pub const TEXTE_2: Color32 = Color32::from_rgb(0x60, 0x5E, 0x5C);
    pub const TEXTE_3: Color32 = Color32::from_rgb(0xA1, 0x9F, 0x9D);
    /// Les filets : des cartes et du dock (`border-gray-200`), des rangées (`-100`), d'un bouton (`-300`), d'un champ.
    pub const FILET: Color32 = Color32::from_rgb(0xED, 0xEB, 0xE9);
    pub const FILET_PALE: Color32 = Color32::from_rgb(0xF3, 0xF2, 0xF1);
    pub const FILET_BOUTON: Color32 = Color32::from_rgb(0xD2, 0xD0, 0xCE);
    pub const FILET_CHAMP: Color32 = Color32::from_rgb(0x8A, 0x88, 0x86);
    /// Le gris d'un survol (`hover:bg-[#f3f2f1]` des boutons secondaires).
    pub const SURVOL: Color32 = Color32::from_rgb(0xF3, 0xF2, 0xF1);
    /// Le texte des réponses de l'assistant (`aiMarkdown.ts` : #334155), son détail (#475569), ses `code` (#F1F5F9),
    /// ses blocs de code — sombres, comme GitHub — et ses filets (#E2E8F0).
    pub const ARDOISE: Color32 = Color32::from_rgb(0x33, 0x41, 0x55);
    pub const ARDOISE_2: Color32 = Color32::from_rgb(0x47, 0x55, 0x69);
    pub const CODE: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);
    pub const BLOC: Color32 = Color32::from_rgb(0x0D, 0x11, 0x17);
    pub const BLOC_FILET: Color32 = Color32::from_rgb(0x30, 0x36, 0x3D);
    pub const BLOC_ENCRE: Color32 = Color32::from_rgb(0xE6, 0xED, 0xF3);
    pub const FILET_FROID: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
    /// Les bulles de l'assistant (`.ai-bubble-assistant`) : leur dégradé à 135°, leur liseré (`border-l-blue-500`) ;
    /// celles de la personne : de `blue-600` à `blue-700`.
    pub const BULLE: [Color32; 3] =
        [Color32::from_rgb(0xFF, 0xFF, 0xFF), Color32::from_rgb(0xF8, 0xFA, 0xFC), Color32::from_rgb(0xF0, 0xF4, 0xFF)];
    pub const BULLE_LISERE: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);
    pub const TOI: [Color32; 2] = [Color32::from_rgb(0x25, 0x63, 0xEB), Color32::from_rgb(0x1D, 0x4E, 0xD8)];
    /// Les états (`seaop-success`, `-warning`, `-danger`) et leurs alertes (`.erp-alert-*`).
    pub const SUCCES: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
    pub const SUCCES_FOND: Color32 = Color32::from_rgb(0xF0, 0xFD, 0xF4);
    pub const SUCCES_ENCRE: Color32 = Color32::from_rgb(0x15, 0x80, 0x3D);
    pub const AVERTISSEMENT: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
    pub const AVERTISSEMENT_FOND: Color32 = Color32::from_rgb(0xFF, 0xFB, 0xEB);
    pub const AVERTISSEMENT_BORD: Color32 = Color32::from_rgb(0xFC, 0xD3, 0x4D);
    pub const AVERTISSEMENT_ENCRE: Color32 = Color32::from_rgb(0x92, 0x40, 0x0E);
    pub const DANGER_ENCRE: Color32 = Color32::from_rgb(0x99, 0x1B, 0x1B);
    /// Le bouton qui confirme : celui de l'Eve de Constructo AI dans son thème clair (`--oui-fond`).
    pub const OUI: Color32 = Color32::from_rgb(0x0B, 0x6E, 0x0B);

    /// Une teinte pastel de l'ERP (`Badge.tsx`, les cartes d'indicateurs d'`AnalyticsPage.tsx`) : sa couleur (à 15 %,
    /// le fond d'un badge ; à 10 %, le haut d'une carte) et son encre ; et son dégradé vif, du clair au soutenu, celui
    /// de la pastille de son icône (les couleurs franches des tableaux de bord d'aujourd'hui).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Teinte {
        pub pastel: Color32,
        pub encre: Color32,
        pub vif: [Color32; 2],
    }

    const fn rgb(c: [u8; 3]) -> Color32 {
        Color32::from_rgb(c[0], c[1], c[2])
    }

    const fn teinte(pastel: [u8; 3], encre: [u8; 3], vif: [[u8; 3]; 2]) -> Teinte {
        Teinte { pastel: rgb(pastel), encre: rgb(encre), vif: [rgb(vif[0]), rgb(vif[1])] }
    }

    pub const BLEU: Teinte = teinte([0x7B, 0xAF, 0xD4], [0x4A, 0x7F, 0xA8], [[0x60, 0xA5, 0xFA], [0x25, 0x63, 0xEB]]);
    pub const VERT: Teinte = teinte([0x7D, 0xC4, 0xA5], [0x4A, 0x94, 0x75], [[0x34, 0xD3, 0x99], [0x05, 0x96, 0x69]]);
    pub const ROUGE: Teinte = teinte([0xE8, 0x91, 0x9A], [0xB8, 0x61, 0x6A], [[0xF8, 0x71, 0x71], [0xDC, 0x26, 0x26]]);
    pub const JAUNE: Teinte = teinte([0xF6, 0xC8, 0x7A], [0x9E, 0x7B, 0x1E], [[0xFC, 0xD3, 0x4D], [0xD9, 0x77, 0x06]]);
    pub const AMBRE: Teinte = teinte([0xE8, 0xC1, 0x7A], [0x8B, 0x70, 0x30], [[0xFB, 0xBF, 0x24], [0xD9, 0x77, 0x06]]);
    pub const ORANGE: Teinte = teinte([0xF0, 0xB0, 0x7A], [0xA0, 0x6A, 0x2A], [[0xFB, 0x92, 0x3C], [0xEA, 0x58, 0x0C]]);
    pub const VIOLET: Teinte = teinte([0xB0, 0x9B, 0xD8], [0x7A, 0x6B, 0xA8], [[0xA7, 0x8B, 0xFA], [0x7C, 0x3A, 0xED]]);
    pub const INDIGO: Teinte = teinte([0x8B, 0x9F, 0xD4], [0x5B, 0x6F, 0xA4], [[0x81, 0x8C, 0xF8], [0x4F, 0x46, 0xE5]]);
    pub const SARCELLE: Teinte =
        teinte([0x7D, 0xC4, 0xB5], [0x4A, 0x94, 0x85], [[0x2D, 0xD4, 0xBF], [0x0D, 0x94, 0x88]]);
    pub const GRIS: Teinte = teinte([0xB8, 0xC4, 0xCE], [0x6B, 0x7B, 0x8A], [[0x94, 0xA3, 0xB8], [0x47, 0x55, 0x69]]);

    /// Le titre d'une carte (le gris ardoise le plus foncé des tableaux de bord) et le filet froid des cartes.
    pub const TITRE: Color32 = Color32::from_rgb(0x1F, 0x29, 0x37);
    pub const FILET_DE_CARTE: Color32 = Color32::from_rgb(0xE5, 0xEA, 0xF1);
    /// Le bleu vif de la bannière, de son milieu à son bout.
    pub const AZUR: Color32 = Color32::from_rgb(0x0B, 0x4A, 0xA6);
    pub const CIEL: Color32 = Color32::from_rgb(0x1E, 0x88, 0xE5);
}

/// Les icônes de Windows du thème (Segoe Fluent Icons), vérifiées sur un échantillon rendu.
pub(crate) mod pictos {
    pub const ECHEANCES: char = '\u{EC92}';
    pub const DECISION: char = '\u{E7C1}';
    pub const COURRIELS: char = '\u{E715}';
    pub const AGENDA: char = '\u{E787}';
    pub const SOUPER: char = '\u{ED56}';
    pub const EPICERIE: char = '\u{EC09}';
    pub const QUESTIONS: char = '\u{E9CE}';
    pub const ENCAISSE: char = '\u{E825}';
    pub const COMPTE: char = '\u{E916}';
    pub const CHIFFRES: char = '\u{E9D2}';
    pub const TEMPETE: char = '\u{E753}';
    pub const CUEILLETTE: char = '\u{E804}';
    pub const PERSONNE: char = '\u{E77B}';
    /// Les rappels : la cloche.
    pub const RAPPELS: char = '\u{EA8F}';
    /// « Arrêter » : Eve se tait (le haut-parleur barré ; le carré de l'arrêt se lirait « agrandir »).
    pub const TAIRE: char = '\u{E74F}';
    pub const SOLEIL: char = '\u{E706}';
    pub const SOLEIL_VOILE: char = '\u{E793}';
    pub const NUAGE: char = '\u{E753}';
}

// ---- Le fond qui respire --------------------------------------------------------------------------------------------

/// Les arrêts du dégradé, à intervalles égaux (`linear-gradient(-45deg, #fff, var(--ui-bg-1), #fff, var(--ui-bg-2),
/// #fff, var(--ui-bg-1))`).
const ARRETS: [Color32; 6] = [Color32::WHITE, erp::FOND_1, Color32::WHITE, erp::FOND_2, Color32::WHITE, erp::FOND_1];
/// L'aller et le retour du fond (`bgBreath 20s`).
const RESPIRATION: f64 = 20.0;
/// Où la fenêtre garde, d'une image à l'autre, le fond de l'image (pour le fondu du fil) et le thème choisi d'un clic.
const FOND: &str = "fond-qui-respire";
const THEME_TENU: &str = "theme-choisi-d-un-clic";
/// Où la fenêtre garde le portrait d'Eve de l'image, pour les bulles et l'en-tête du dock.
const PORTRAIT: &str = "portrait-d-eve-du-fil";

/// Le portrait d'Eve gardé pour cette image ([`Fenetre::garder_le_portrait`]) ; `None` sans portrait.
fn portrait_du_fil(ctx: &egui::Context) -> Option<portrait::Visage> {
    ctx.data(|d| d.get_temp::<Option<portrait::Visage>>(egui::Id::unique(PORTRAIT))).flatten()
}

/// La courbe `ease` de CSS, `cubic-bezier(0.25, 0.1, 0.25, 1)` : la part de la course faite à la part `x` du temps.
fn ease(x: f64) -> f64 {
    bezier((0.25, 0.1), (0.25, 1.0), x)
}

/// Une courbe de Bézier cubique de CSS, de (0, 0) à (1, 1) par `a` et `b` : son ordonnée à l'abscisse `x` (Newton,
/// puis la dichotomie si la pente s'aplatit, comme les navigateurs).
fn bezier(a: (f64, f64), b: (f64, f64), x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let coefficients = |p: f64, q: f64| {
        let c = 3.0 * p;
        let b = 3.0 * (q - p) - c;
        (1.0 - c - b, b, c)
    };
    let ((ax, bx, cx), (ay, by, cy)) = (coefficients(a.0, b.0), coefficients(a.1, b.1));
    let abscisse = |t: f64| ((ax * t + bx) * t + cx) * t;
    let ordonnee = |t: f64| ((ay * t + by) * t + cy) * t;
    let mut t = x;
    for _ in 0..8 {
        let ecart = abscisse(t) - x;
        if ecart.abs() < 1e-7 {
            return ordonnee(t);
        }
        let pente = (3.0 * ax * t + 2.0 * bx) * t + cx;
        if pente.abs() < 1e-6 {
            break;
        }
        t -= ecart / pente;
    }
    let (mut bas, mut haut) = (0.0, 1.0);
    t = x;
    for _ in 0..60 {
        let v = abscisse(t);
        if (v - x).abs() < 1e-7 {
            break;
        }
        if v < x {
            bas = t;
        } else {
            haut = t;
        }
        t = (bas + haut) / 2.0;
    }
    ordonnee(t)
}

/// La course du fond à l'instant `t` : son `background-position` en x, de 0 (0 %) à 1 (100 %). L'aller en 10 s, le
/// retour en 10 s, chacun sur la courbe `ease` (elle vaut pour chaque intervalle entre deux images clés de `bgBreath`).
fn course(t: f64) -> f32 {
    let p = (t / RESPIRATION).rem_euclid(1.0) * 2.0;
    (if p < 1.0 { ease(p) } else { 1.0 - ease(p - 1.0) }) as f32
}

/// La couleur du dégradé à sa position `t` (0, son début ; 1, sa fin), entre ses arrêts, en sRGB comme CSS (et comme
/// egui mélange les couleurs des sommets).
fn couleur_du_degrade(t: f32) -> Color32 {
    let x = t.clamp(0.0, 1.0) * (ARRETS.len() - 1) as f32;
    let i = (x.floor() as usize).min(ARRETS.len() - 2);
    melange(ARRETS[i], ARRETS[i + 1], x - i as f32)
}

/// Le fond d'une image : la zone où il est posé (celle de `background-attachment: fixed` : l'image du dégradé en fait
/// quatre fois la taille) et sa course.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fond {
    zone: Rect,
    course: f32,
}

impl Fond {
    /// La position dans le dégradé du point `p` : 0 au coin en bas à droite de l'image, 1 au coin en haut à gauche (un
    /// angle de −45°). L'image, de 4L × 4H, est décalée de −3L × la course et de −1,5H (`background-position: X%
    /// 50%`) ; sa ligne de dégradé mesure 4(L + H) / √2, et `t` ne dépend que de x + y.
    fn t(&self, p: Pos2) -> f32 {
        let (l, h) = (self.zone.width().max(1.0), self.zone.height().max(1.0));
        let q = p - self.zone.min;
        0.5 - (q.x + q.y + 3.0 * l * self.course - 2.0 * l - 0.5 * h) / (4.0 * (l + h))
    }

    fn couleur(&self, p: Pos2) -> Color32 {
        couleur_du_degrade(self.t(p))
    }

    /// Le maillage du fond sur sa zone : une bande entre deux arrêts du dégradé (les droites x + y = constante), que la
    /// zone découpe ; dans une bande, la couleur va en ligne droite, et ses sommets la portent exactement.
    fn maillage(&self) -> Mesh {
        let z = self.zone;
        let (l, h) = (z.width().max(1.0), z.height().max(1.0));
        // Où chaque arrêt tombe, en x + y depuis le coin en haut à gauche : t = t(0) − s / 4(L + H).
        let t0 = self.t(z.min);
        let mut coupes: Vec<f32> = (0..ARRETS.len())
            .map(|k| (t0 - k as f32 / (ARRETS.len() - 1) as f32) * 4.0 * (l + h))
            .filter(|s| *s > 0.0 && *s < l + h)
            .collect();
        coupes.extend([0.0, l + h]);
        coupes.sort_by(f32::total_cmp);
        let coins = [z.left_top(), z.right_top(), z.right_bottom(), z.left_bottom()];
        let s = |p: Pos2| (p.x - z.left()) + (p.y - z.top());
        let mut m = Mesh::default();
        for paire in coupes.windows(2) {
            let (de, a) = (paire[0], paire[1]);
            let bande = couper(&couper(&coins, |p| s(p) - de), |p| a - s(p));
            if bande.len() < 3 {
                continue;
            }
            let premier = m.vertices.len() as u32;
            for p in &bande {
                m.colored_vertex(*p, self.couleur(*p));
            }
            for k in 1..bande.len() as u32 - 1 {
                m.add_triangle(premier, premier + k, premier + k + 1);
            }
        }
        m
    }
}

/// Un polygone convexe coupé par le demi-plan où `garder` est positive (une fonction affine) : Sutherland-Hodgman.
fn couper(points: &[Pos2], garder: impl Fn(Pos2) -> f32) -> Vec<Pos2> {
    let mut sortie = Vec::with_capacity(points.len() + 2);
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let (fa, fb) = (garder(a), garder(b));
        if fa >= 0.0 {
            sortie.push(a);
        }
        if (fa >= 0.0) != (fb >= 0.0) {
            sortie.push(a + (b - a) * (fa / (fa - fb)));
        }
    }
    sortie
}

/// Windows veut des animations (Paramètres, Accessibilité, Effets d'animation) ; sinon le fond se fige au milieu de sa
/// course, comme celui de l'ERP sous `prefers-reduced-motion`. Relu toutes les dix secondes.
fn animations(ctx: &egui::Context) -> bool {
    let id = egui::Id::unique("animations-de-windows");
    let maintenant = ctx.input(|i| i.time);
    if let Some((oui, lu)) = ctx.data(|d| d.get_temp::<(bool, f64)>(id))
        && maintenant - lu < 10.0
    {
        return oui;
    }
    let oui = animations_de_windows();
    ctx.data_mut(|d| d.insert_temp(id, (oui, maintenant)));
    oui
}

#[cfg(windows)]
fn animations_de_windows() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut oui: i32 = 1;
    let pointeur = (&raw mut oui).cast::<core::ffi::c_void>();
    // SAFETY : SPI_GETCLIENTAREAANIMATION écrit un BOOL (quatre octets) dans `oui`, qui vit jusqu'au retour.
    let lu = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(pointeur), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    lu.is_err() || oui != 0
}

#[cfg(not(windows))]
fn animations_de_windows() -> bool {
    true
}

/// Le fondu du haut du fil défilé, de la couleur du fond là où il est (le fond respire : pas de couleur unie).
pub(super) fn fondu(p: &egui::Painter, rect: Rect) {
    let Some(fond) = p.ctx().data(|d| d.get_temp::<Fond>(egui::Id::unique(FOND))) else { return };
    let mut m = Mesh::default();
    let n = 8;
    for i in 0..=n {
        let x = egui::lerp(rect.x_range(), i as f32 / n as f32);
        let haut = pos2(x, rect.top());
        m.colored_vertex(haut, fond.couleur(haut));
        m.colored_vertex(pos2(x, rect.bottom()), Color32::TRANSPARENT);
        if i > 0 {
            let k = 2 * i as u32;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(Shape::mesh(m));
}

// ---- Les visuels, les encres ------------------------------------------------------------------------------------

/// Les visuels d'egui dans le thème clair : le mode clair (l'orbe le lit, et les encres du texte d'Eve), le texte, la
/// sélection (`::selection` de l'ERP : #DBEAFE et #1E40AF), le curseur, les barres de défilement (celles de l'ERP :
/// #CBD5E1, #94A3B8 au survol), la poignée du dock (`hover:bg-blue-500/20`).
fn visuels_clairs(v: &mut egui::Visuals) {
    let ardoise = Color32::from_rgb(0xCB, 0xD5, 0xE1);
    let ardoise_survol = Color32::from_rgb(0x94, 0xA3, 0xB8);
    v.dark_mode = false;
    v.override_text_color = None;
    v.weak_text_color = Some(erp::TEXTE_2);
    v.hyperlink_color = erp::PRIMAIRE;
    v.faint_bg_color = erp::FILET_PALE;
    v.extreme_bg_color = Color32::WHITE;
    v.text_edit_bg_color = Some(Color32::WHITE);
    v.code_bg_color = erp::CODE;
    v.panel_fill = Color32::TRANSPARENT;
    v.selection.bg_fill = Color32::from_rgb(0xDB, 0xEA, 0xFE);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0x1E, 0x40, 0xAF));
    v.text_cursor.stroke = Stroke::new(2.0, erp::PRIMAIRE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, erp::TEXTE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, erp::FILET);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, erp::TEXTE);
    v.widgets.inactive.bg_fill = ardoise;
    v.widgets.inactive.weak_bg_fill = ardoise;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, erp::TEXTE);
    v.widgets.hovered.bg_fill = ardoise_survol;
    v.widgets.hovered.weak_bg_fill = ardoise_survol;
    v.widgets.hovered.bg_stroke = Stroke::new(2.0, erp::BULLE_LISERE.gamma_multiply(0.35));
    v.widgets.active.fg_stroke = Stroke::new(1.0, erp::TEXTE);
    v.widgets.active.bg_fill = ardoise_survol;
    v.widgets.active.weak_bg_fill = ardoise_survol;
    v.widgets.active.bg_stroke = Stroke::new(2.0, erp::BULLE_LISERE.gamma_multiply(0.6));
}

/// Les encres du texte d'Eve (`texte_d_eve`) : celles du thème sombre, ou celles du clair, comme le markdown de
/// l'assistant de l'ERP (`aiMarkdown.ts`). La fenêtre prend celles du `Ui` ([`encres`]).
#[derive(Clone, Copy)]
pub(super) struct Encres {
    /// Le texte ; le détail écrit après « --- » ; un titre (« # ») ; un lien ; une puce.
    pub(super) texte: Color32,
    pub(super) detail: Color32,
    pub(super) titre: Color32,
    pub(super) accent: Color32,
    pub(super) puce: Color32,
    /// Un `code` dans une phrase : son fond et son encre.
    pub(super) code_fond: Color32,
    pub(super) code_encre: Color32,
    /// Un bloc de code : son fond, son filet, son encre.
    pub(super) bloc_fond: Color32,
    pub(super) bloc_filet: Color32,
    pub(super) bloc_encre: Color32,
    /// Le filet de « --- ».
    pub(super) filet: Color32,
}

pub(super) const SOMBRES: Encres = Encres {
    texte: couleurs::ENCRE,
    detail: couleurs::ENCRE_2,
    titre: couleurs::ENCRE,
    accent: couleurs::ACCENT,
    puce: couleurs::ACCENT,
    code_fond: couleurs::FILET,
    code_encre: couleurs::ENCRE,
    bloc_fond: couleurs::FOND,
    bloc_filet: couleurs::FILET,
    bloc_encre: couleurs::ENCRE_2,
    filet: couleurs::FILET_FORT,
};

const CLAIRES: Encres = Encres {
    texte: erp::ARDOISE,
    detail: erp::ARDOISE_2,
    titre: erp::MARINE,
    accent: erp::PRIMAIRE,
    puce: erp::MARINE,
    code_fond: erp::CODE,
    code_encre: erp::MARINE,
    bloc_fond: erp::BLOC,
    bloc_filet: erp::BLOC_FILET,
    bloc_encre: erp::BLOC_ENCRE,
    filet: erp::FILET_FROID,
};

/// Les encres du texte d'Eve dans ce `Ui` : claires dans un `Ui` aux visuels clairs (le thème « Constructo AI »).
pub(super) fn encres(ui: &egui::Ui) -> &'static Encres {
    if ui.visuals().dark_mode { &SOMBRES } else { &CLAIRES }
}

/// Une couleur qui se lit sur du blanc (un contraste de 4,5 au moins, celui du WCAG) : elle-même, ou assombrie vers
/// l'encre du texte jusqu'à ce qu'elle se lise — la couleur d'une personne garde sa teinte.
pub(crate) fn lisible(c: Color32) -> Color32 {
    (0..=20)
        .map(|k| melange(c, erp::TEXTE, k as f32 * 0.05))
        .find(|x| contraste(*x, Color32::WHITE) >= 4.5)
        .unwrap_or(erp::TEXTE)
}

/// Le contraste de deux couleurs, selon le WCAG : de 1 (le même) à 21 (le noir sur le blanc).
fn contraste(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let l = |x: u8| {
            let v = f32::from(x) / 255.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * l(c.r()) + 0.7152 * l(c.g()) + 0.0722 * l(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

// ---- Les pièces de l'ERP -------------------------------------------------------------------------------------------

/// Une carte du tableau de bord, comme celles des tableaux de bord d'aujourd'hui : blanche, un rayon de 14 points, un
/// filet froid, deux ombres douces teintées de marine (plus marquées au survol, la carte semble se lever) ; sa couleur
/// (`accent`) lave doucement son haut. Elle prend toute la largeur donnée et au moins `hauteur` (une rangée d'une
/// grille : ses cartes ont la même hauteur) ; rend la hauteur qu'il lui faut.
pub(crate) fn carte(
    ui: &mut egui::Ui,
    accent: Option<Color32>,
    hauteur: f32,
    contenu: impl FnOnce(&mut egui::Ui),
) -> f32 {
    let fond = ui.painter().add(Shape::Noop);
    let (coin, largeur) = (ui.cursor().min, ui.available_width());
    let marge = Margin { left: 18, right: 18, top: 16, bottom: 17 };
    let dedans = Rect::from_min_max(
        coin + vec2(f32::from(marge.left), f32::from(marge.top)),
        pos2(coin.x + largeur - f32::from(marge.right), coin.y + 100_000.0),
    );
    let mut enfant = ui.new_child(UiBuilder::new().max_rect(dedans).layout(Layout::top_down(Align::Min)));
    enfant.spacing_mut().item_spacing.y = 6.0;
    contenu(&mut enfant);
    let naturelle = enfant.min_rect().height() + f32::from(marge.top) + f32::from(marge.bottom);
    let rect = Rect::from_min_size(coin, vec2(largeur, naturelle.max(hauteur)));
    ui.advance_cursor_after_rect(rect);
    let survol = ui.rect_contains_pointer(rect);
    ui.painter().set(fond, fond_de_carte(rect, accent, survol));
    naturelle
}

/// Le rayon des cartes et de la bannière.
const RAYON_DE_CARTE: u8 = 14;

/// Une ombre teintée de marine, plus douce qu'une ombre noire : `dy` points plus bas, floue de `flou`, à `alpha`.
fn ombre_marine(rect: Rect, rayon: u8, dy: i8, flou: u8, alpha: u8) -> Shape {
    let couleur = Color32::from_rgba_unmultiplied(15, 35, 80, alpha);
    Shape::Rect(egui::Shadow { offset: [0, dy], blur: flou, spread: 0, color: couleur }.as_shape(rect, rayon))
}

/// Le fond d'une carte : ses deux ombres (au survol, plus basses et plus larges), sa face blanche et son filet ; le
/// haut lavé de sa couleur, de 10 % à rien sur 64 points.
fn fond_de_carte(rect: Rect, accent: Option<Color32>, survol: bool) -> Shape {
    let rayon = RAYON_DE_CARTE;
    let mut formes = vec![ombre_marine(rect, rayon, 1, 3, 14)];
    formes.push(if survol { ombre_marine(rect, rayon, 10, 28, 38) } else { ombre_marine(rect, rayon, 6, 20, 22) });
    formes.push(Shape::Rect(egui::epaint::RectShape::new(
        rect,
        rayon,
        Color32::WHITE,
        Stroke::new(1.0, erp::FILET_DE_CARTE),
        StrokeKind::Inside,
    )));
    if let Some(c) = accent {
        let haut = Rect::from_min_size(rect.min, vec2(rect.width(), 64.0f32.min(rect.height()))).shrink(1.0);
        let lavis = |p: Pos2| c.gamma_multiply(0.1 * (1.0 - ((p.y - haut.top()) / haut.height()).clamp(0.0, 1.0)));
        formes.push(rectangle_degrade(haut, f32::from(rayon) - 1.0, lavis));
    }
    Shape::Vec(formes)
}

/// Une pastille d'icône, comme celles des applications : un carré aux coins arrondis, en dégradé de la teinte (du
/// clair, en haut à gauche, au soutenu), son icône blanche.
pub(crate) fn pastille(p: &egui::Painter, case: Rect, icone: char, teinte: erp::Teinte) {
    let [clair, soutenu] = teinte.vif;
    let degrade = |q: Pos2| {
        let t = ((q.x - case.left()) + (q.y - case.top())) / (case.width() + case.height()).max(1.0);
        melange(clair, soutenu, t.clamp(0.0, 1.0))
    };
    p.add(ombre_marine(case, 10, 2, 6, 30));
    p.add(rectangle_degrade(case, case.width() * 0.3, degrade));
    let police = FontId::new(case.width() * 0.47, FontFamily::Name(ICONES.into()));
    p.text(case.center(), Align2::CENTER_CENTER, icone, police, Color32::WHITE);
}

/// Un texte du tableau en capitales (« ÉPICERIE · 0 À ACHETER », « À SORTIR : poulet ») en casse de phrase
/// (« Épicerie · 0 à acheter », « À sortir : poulet ») : chaque mot tout en capitales passe en minuscules, un mot qui a
/// ses minuscules (un nom, une donnée) reste tel quel ; puis la première lettre en capitale.
pub(crate) fn casse_de_phrase(texte: &str) -> String {
    let mots: Vec<String> = texte
        .split(' ')
        .map(|m| if m.chars().any(char::is_lowercase) { m.to_string() } else { m.to_lowercase() })
        .collect();
    let phrase = mots.join(" ");
    let mut c = phrase.chars();
    c.next().map_or_else(String::new, |p| p.to_uppercase().collect::<String>() + c.as_str())
}

/// L'écart entre deux cartes du tableau de bord, et la largeur sous laquelle une carte ne descend pas dans la grille.
const ECART: f32 = 20.0;
const LARGEUR_DE_CARTE: f32 = 250.0;

/// Le nombre de colonnes de la grille dans une largeur `l` : autant de cartes qu'il en tient, de une à trois.
pub(crate) fn colonnes_pour(l: f32) -> usize {
    ((l + ECART) / (LARGEUR_DE_CARTE + ECART)).floor().clamp(1.0, 3.0) as usize
}

/// Une grille de cartes de même hauteur par rangée (celles du tableau de bord de l'ERP, `grid-cols-N gap-4`) : `n`
/// cases sur `colonnes` colonnes, à [`ECART`] l'une de l'autre. `case(ui, k, hauteur)` dessine la k-ième dans sa case,
/// au moins à `hauteur` (celle de la plus haute de sa rangée, mesurée à l'image d'avant), et rend celle qu'il lui faut.
pub(crate) fn grille(
    ui: &mut egui::Ui,
    id: egui::Id,
    colonnes: usize,
    n: usize,
    mut case: impl FnMut(&mut egui::Ui, usize, f32) -> f32,
) {
    if n == 0 {
        return;
    }
    let (ecart, colonnes) = (ECART, colonnes.max(1));
    let (origine, largeur) = (ui.cursor().min, ui.available_width());
    let l = ((largeur - ecart * (colonnes - 1) as f32) / colonnes as f32).max(40.0);
    let avant: Vec<f32> = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    let mut apres = vec![0.0; n];
    let mut y = origine.y;
    for debut in (0..n).step_by(colonnes) {
        let fin = (debut + colonnes).min(n);
        let h = (debut..fin).filter_map(|k| avant.get(k).copied()).fold(0.0, f32::max);
        let mut bas = y;
        for k in debut..fin {
            let x = origine.x + (k - debut) as f32 * (l + ecart);
            let place = Rect::from_min_size(pos2(x, y), vec2(l, 100_000.0));
            let mut enfant = ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)));
            apres[k] = case(&mut enfant, k, h);
            bas = bas.max(enfant.min_rect().bottom());
        }
        y = bas + ecart;
    }
    ui.advance_cursor_after_rect(Rect::from_min_max(origine, pos2(origine.x + largeur, y - ecart)));
    let change = apres.len() != avant.len() || apres.iter().zip(&avant).any(|(a, b)| (a - b).abs() > 0.5);
    if change {
        ui.ctx().data_mut(|d| d.insert_temp(id, apres));
        ui.ctx().request_repaint();
    }
}

/// Le texte d'un badge de l'ERP (`Badge.tsx`), en demi-gras de l'encre de sa teinte, mis en page (sa place : sa
/// taille, plus 18 points de large et 7 de haut).
pub(crate) fn galee_de_badge(p: &egui::Painter, texte: &str, teinte: erp::Teinte) -> std::sync::Arc<egui::Galley> {
    p.layout_no_wrap(texte.to_string(), FontId::new(11.5, FontFamily::Name(SEMI.into())), teinte.encre)
}

/// Un badge peint dans `rect`, sa pilule et son texte au milieu.
pub(crate) fn peindre_badge(p: &egui::Painter, rect: Rect, galee: std::sync::Arc<egui::Galley>, teinte: erp::Teinte) {
    p.rect_filled(rect, 255.0, teinte.pastel.gamma_multiply(0.2));
    p.galley(rect.center() - galee.size() / 2.0, galee, teinte.encre);
}

/// La tête d'une carte : la pastille de son icône ([`pastille`]), puis son titre en demi-gras, en casse de phrase
/// ([`casse_de_phrase`]), qui passe à la ligne au besoin ; en alerte, la pastille et le titre rouges. `etiquette` : un
/// badge contre le bord droit (le délai de la décision du jour).
pub(crate) fn tete_de_carte(
    ui: &mut egui::Ui,
    titre: &str,
    icone: char,
    teinte: erp::Teinte,
    alerte: bool,
    etiquette: Option<(&str, erp::Teinte)>,
) {
    let largeur = ui.available_width();
    let cote = 36.0;
    let (teinte, encre) = if alerte { (erp::ROUGE, erp::DANGER_ENCRE) } else { (teinte, erp::TITRE) };
    let p = ui.painter();
    let badge = etiquette.map(|(t, x)| (galee_de_badge(p, &casse_de_phrase(t), x), x));
    let l_badge = badge.as_ref().map_or(0.0, |(g, _)| g.size().x + 18.0 + 12.0);
    let police = FontId::new(15.0, FontFamily::Name(SEMI.into()));
    let galee = p.layout(casse_de_phrase(titre), police, encre, (largeur - cote - 12.0 - l_badge).max(40.0));
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, galee.size().y.max(cote)), Sense::hover());
    let p = ui.painter();
    let case = Rect::from_min_size(pos2(rect.left(), rect.center().y - cote / 2.0), vec2(cote, cote));
    pastille(p, case, icone, teinte);
    p.galley(pos2(case.right() + 12.0, rect.center().y - galee.size().y / 2.0), galee, encre);
    if let Some((g, x)) = badge {
        let taille = g.size() + vec2(18.0, 7.0);
        let place = Rect::from_min_size(pos2(rect.right() - taille.x, rect.center().y - taille.y / 2.0), taille);
        peindre_badge(p, place, g, x);
    }
    ui.add_space(4.0);
}

/// Une page de calendrier, celle des agendas : le mois en capitales blanches sur un bandeau de la teinte, le jour en
/// gras dessous, sur une page blanche au filet froid et à l'ombre douce.
pub(crate) fn page_de_calendrier(p: &egui::Painter, r: Rect, jour: &str, mois: &str, teinte: erp::Teinte) {
    let rayon = 10;
    p.add(ombre_marine(r, rayon, 2, 6, 26));
    p.rect(r, rayon, Color32::WHITE, Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let bandeau = Rect::from_min_size(r.min, vec2(r.width(), 17.0));
    let coins = CornerRadius { nw: rayon, ne: rayon, sw: 0, se: 0 };
    p.rect_filled(bandeau, coins, teinte.vif[1]);
    let petite = FontId::new(9.5, FontFamily::Name(SEMI.into()));
    p.text(bandeau.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, mois, petite, Color32::WHITE);
    let page = Rect::from_min_max(pos2(r.left(), bandeau.bottom()), r.max);
    let grande = FontId::new(20.0, FontFamily::Name(GRAS.into()));
    p.text(page.center() - vec2(0.0, 0.5), Align2::CENTER_CENTER, jour, grande, erp::TITRE);
}

/// Les sortes de boutons de l'ERP : celui qui confirme (vert, celui de l'Eve de Constructo AI), secondaire
/// (`.erp-btn-secondary`), fantôme (`.erp-btn-ghost`). Le primaire (`.erp-btn-primary`), c'est l'envoi.
#[derive(Clone, Copy, PartialEq)]
enum BoutonErp {
    Oui,
    Secondaire,
    Fantome,
}

/// Un bouton de l'ERP : un rayon de 4 points, une icône au besoin, son mot en demi-gras.
fn bouton_erp(ui: &mut egui::Ui, code: Option<char>, texte: &str, sorte: BoutonErp) -> egui::Response {
    let (taille, px, py) = if sorte == BoutonErp::Fantome { (12.5, 10.0, 6.0) } else { (13.5, 16.0, 8.0) };
    // Sans couleur à la mise en page : l'encre se donne à la peinture (le survol la change).
    let (p, aucune) = (ui.painter(), Color32::PLACEHOLDER);
    let g_texte = p.layout_no_wrap(texte.to_string(), FontId::new(taille, FontFamily::Name(SEMI.into())), aucune);
    let g_icone =
        code.map(|c| p.layout_no_wrap(c.to_string(), FontId::new(taille, FontFamily::Name(ICONES.into())), aucune));
    let l_icone = g_icone.as_ref().map_or(0.0, |g| g.size().x + 7.0);
    let (rect, rep) = ui
        .allocate_exact_size(vec2(2.0 * px + l_icone + g_texte.size().x, 2.0 * py + g_texte.size().y), Sense::click());
    if ui.is_rect_visible(rect) {
        let (survol, enfonce) = (rep.hovered(), rep.is_pointer_button_down_on());
        let (fond, bord, encre) = match sorte {
            BoutonErp::Oui => {
                let c = if enfonce { melange(erp::OUI, Color32::BLACK, 0.15) } else { erp::OUI };
                (
                    if survol && !enfonce { melange(c, Color32::WHITE, 0.08) } else { c },
                    Color32::TRANSPARENT,
                    Color32::WHITE,
                )
            }
            BoutonErp::Secondaire => (if survol { erp::SURVOL } else { Color32::WHITE }, erp::FILET_CHAMP, erp::TEXTE),
            BoutonErp::Fantome => (
                if survol { erp::SURVOL } else { Color32::TRANSPARENT },
                Color32::TRANSPARENT,
                if survol { erp::TEXTE } else { erp::TEXTE_2 },
            ),
        };
        let p = ui.painter();
        p.rect(rect, 4.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
        let mut x = rect.left() + px;
        if let Some(g) = g_icone {
            let l = g.size().x;
            p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, encre);
            x += l + 7.0;
        }
        p.galley(pos2(x, rect.center().y - g_texte.size().y / 2.0), g_texte, encre);
    }
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Un rectangle aux coins arrondis peint d'un dégradé : ses sommets prennent la couleur de `couleur` à leur place (un
/// éventail depuis le centre ; ses côtés droits coupés tous les 24 points, pour qu'un dégradé à trois arrêts y garde
/// celui du milieu). Pour des dégradés doux, comme ceux des bulles de l'ERP.
fn rectangle_degrade(rect: Rect, rayon: f32, couleur: impl Fn(Pos2) -> Color32) -> Shape {
    let rayon = rayon.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let centres = [
        (pos2(rect.right() - rayon, rect.top() + rayon), -90.0f32),
        (pos2(rect.right() - rayon, rect.bottom() - rayon), 0.0),
        (pos2(rect.left() + rayon, rect.bottom() - rayon), 90.0),
        (pos2(rect.left() + rayon, rect.top() + rayon), 180.0),
    ];
    let mut tour = Vec::with_capacity(20);
    for (c, depart) in centres {
        for k in 0..=4 {
            let a = (depart + k as f32 * 22.5).to_radians();
            tour.push(c + vec2(a.cos(), a.sin()) * rayon);
        }
    }
    let mut m = Mesh::default();
    m.colored_vertex(rect.center(), couleur(rect.center()));
    for (k, &a) in tour.iter().enumerate() {
        let b = tour[(k + 1) % tour.len()];
        let morceaux = ((b - a).length() / 24.0).ceil().max(1.0) as usize;
        for i in 0..morceaux {
            let p = a + (b - a) * (i as f32 / morceaux as f32);
            m.colored_vertex(p, couleur(p));
        }
    }
    let n = m.vertices.len() as u32 - 1;
    for k in 1..=n {
        m.add_triangle(0, k, if k == n { 1 } else { k + 1 });
    }
    Shape::mesh(m)
}

/// La couleur d'un dégradé linéaire de CSS (`linear-gradient(<angle>, …)`) dans `rect` au point `p` : ses arrêts à
/// intervalles égaux, l'angle en degrés (0, vers le haut ; 90, vers la droite).
fn degrade_css(rect: Rect, angle: f32, arrets: &[Color32], p: Pos2) -> Color32 {
    let (s, c) = angle.to_radians().sin_cos();
    let longueur = (rect.width() * s).abs() + (rect.height() * c).abs();
    let t = ((p - rect.center()).dot(vec2(s, -c)) / longueur.max(1.0) + 0.5).clamp(0.0, 1.0);
    let x = t * (arrets.len() - 1) as f32;
    let i = (x.floor() as usize).min(arrets.len() - 2);
    melange(arrets[i], arrets[i + 1], x - i as f32)
}

// ---- Le fil, en bulles claires ---------------------------------------------------------------------------------

/// Une ligne du fil comme les bulles de l'assistant de l'ERP (`MessageBubble.tsx`) : Eve à gauche, sa petite orbe
/// devant, sa bulle blanche en dégradé au liseré bleu ; la personne à droite, sa bulle en dégradé bleu, sa tuile
/// grise ; Eve-même (le système) au centre, en gris ; une alerte dans le cadre ambre des avertissements de l'ERP. Une
/// ligne d'une séance précédente, plus pâle.
pub(super) fn bulle(ui: &mut egui::Ui, l: &Ligne<'_>, cartes: &mut Cartes, gestes: &mut Vec<Geste>) {
    let Ligne { de, texte, heure, ancien, pieces, en_cours, tour } = *l;
    // Plus pâle, une ligne d'une séance précédente se lit encore sur le fond clair.
    let opacite = if ancien { 0.75 } else { 1.0 };
    let largeur = ui.available_width();
    match de {
        "systeme" => {
            ui.vertical_centered(|ui| {
                ui.set_max_width(largeur * 0.9);
                ui.label(RichText::new(texte).size(12.0).color(erp::TEXTE_2.gamma_multiply(opacite)));
            });
            ui.add_space(12.0);
        }
        "toi" => {
            let max = ((largeur - 46.0) * 0.8).max(120.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                tuile_de_la_personne(ui, opacite);
                ui.with_layout(Layout::top_down(Align::Max), |ui| {
                    let fond = ui.painter().add(Shape::Noop);
                    let r = egui::Frame::new()
                        .inner_margin(Margin::symmetric(16, 11))
                        .show(ui, |ui| {
                            ui.set_max_width(max);
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                ui.label(RichText::new(texte).size(14.0).color(Color32::WHITE.gamma_multiply(opacite)));
                                pieces_jointes(ui, pieces, Color32::from_white_alpha(210), opacite);
                            });
                        })
                        .response
                        .rect;
                    let couleur = |p: Pos2| degrade_css(r, 135.0, &erp::TOI, p).gamma_multiply(opacite);
                    let ombre = egui::Shadow {
                        offset: [0, 4],
                        blur: 12,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(37, 99, 235, 45),
                    };
                    ui.painter().set(
                        fond,
                        Shape::Vec(vec![ombre.as_shape(r, RAYON_DE_BULLE).into(), rectangle_degrade(r, 14.0, couleur)]),
                    );
                    heure_sous(ui, heure, opacite, None, "");
                });
            });
            ui.add_space(4.0);
        }
        _ => {
            let alerte = de == "alerte";
            let max = ((largeur - 46.0) * 0.92).max(120.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                avatar(ui, alerte, opacite);
                ui.vertical(|ui| {
                    let fond = ui.painter().add(Shape::Noop);
                    let r = egui::Frame::new()
                        .inner_margin(Margin { left: 18, right: 16, top: 12, bottom: 13 })
                        .show(ui, |ui| {
                            ui.set_max_width(max);
                            // Une bulle qui porte une carte prend toute sa largeur : la carte s'y étale.
                            if texte.contains("```") {
                                ui.set_width(max);
                            }
                            super::texte_d_eve(ui, texte, opacite, en_cours, cartes, gestes);
                            pieces_jointes(ui, pieces, erp::TEXTE_2, opacite);
                        })
                        .response
                        .rect;
                    ui.painter().set(fond, fond_de_bulle(r, alerte, opacite));
                    heure_sous(ui, heure, opacite, (!en_cours).then_some(texte), tour);
                });
            });
            ui.add_space(4.0);
        }
    }
}

/// Le rayon des bulles.
const RAYON_DE_BULLE: u8 = 14;

/// Le fond d'une bulle d'Eve : son ombre douce teintée de marine, sa face blanche à peine bleutée vers le bas (le
/// dégradé des bulles de l'assistant de l'ERP), son filet froid ; une alerte, dans le cadre ambre de l'ERP
/// (`.erp-alert-warning`).
fn fond_de_bulle(r: Rect, alerte: bool, opacite: f32) -> Shape {
    let rayon = f32::from(RAYON_DE_BULLE);
    let (face, filet) = if alerte {
        (rectangle_degrade(r, rayon, |_| erp::AVERTISSEMENT_FOND.gamma_multiply(opacite)), erp::AVERTISSEMENT_BORD)
    } else {
        let face = rectangle_degrade(r, rayon, |p| degrade_css(r, 160.0, &erp::BULLE, p).gamma_multiply(opacite));
        (face, erp::FILET_DE_CARTE)
    };
    let trait_ = Stroke::new(1.0, filet.gamma_multiply(opacite));
    let contour = egui::epaint::RectShape::stroke(r, RAYON_DE_BULLE, trait_, StrokeKind::Inside);
    let alpha = (20.0 * opacite) as u8;
    Shape::Vec(vec![
        ombre_marine(r, RAYON_DE_BULLE, 1, 3, alpha / 2),
        ombre_marine(r, RAYON_DE_BULLE, 4, 14, alpha),
        face,
        Shape::Rect(contour),
    ])
}

/// La petite orbe d'Eve devant ses bulles, ou son portrait (son ombre douce dessous) ; une cloche ambre pour une alerte.
fn avatar(ui: &mut egui::Ui, alerte: bool, opacite: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
    let (c, p) = (rect.center(), ui.painter());
    if alerte {
        p.rect_filled(rect, 4.0, erp::AVERTISSEMENT_FOND.gamma_multiply(opacite));
        p.rect_stroke(rect, 4.0, Stroke::new(1.0, erp::AVERTISSEMENT_BORD.gamma_multiply(opacite)), StrokeKind::Inside);
        let police = FontId::new(14.0, FontFamily::Name(ICONES.into()));
        p.text(c, Align2::CENTER_CENTER, icones::CLOCHE, police, erp::AVERTISSEMENT_ENCRE.gamma_multiply(opacite));
        return;
    }
    p.circle_filled(c + vec2(0.0, 1.5), 15.0, Color32::from_black_alpha((26.0 * opacite) as u8));
    match portrait_du_fil(ui.ctx()) {
        Some(v) => v.vignette(p, c, 15.0),
        None => sphere(p, c, 15.0),
    }
    p.circle_stroke(c, 15.0, Stroke::new(1.0, Color32::from_white_alpha(70)));
    if opacite < 1.0 {
        p.circle_filled(c, 15.5, Color32::WHITE.gamma_multiply(1.0 - opacite));
    }
}

/// La tuile de la personne, devant ses bulles (`bg-gray-200`, l'icône d'une personne en gris).
fn tuile_de_la_personne(ui: &mut egui::Ui, opacite: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, erp::FILET.gamma_multiply(opacite));
    let police = FontId::new(15.0, FontFamily::Name(ICONES.into()));
    p.text(rect.center(), Align2::CENTER_CENTER, pictos::PERSONNE, police, erp::TEXTE_2.gamma_multiply(opacite));
}

/// L'heure sous une bulle ; sous une réponse d'Eve finie (`copie` : son texte), ce que son tour a pris (`tour`) et
/// « Copier » à côté ([`super::ce_qu_a_pris`], [`super::bouton_copier`]).
fn heure_sous(ui: &mut egui::Ui, heure: &str, opacite: f32, copie: Option<&str>, tour: &str) {
    let encre = erp::TEXTE_3.gamma_multiply(opacite);
    if let Some(reponse) = copie {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if !heure.is_empty() {
                ui.label(RichText::new(heure).size(11.0).color(encre));
            }
            super::ce_qu_a_pris(ui, tour, encre, FontId::proportional(11.0));
            super::bouton_copier(ui, reponse, encre, erp::PRIMAIRE, FontId::proportional(11.0));
        });
    } else if !heure.is_empty() {
        ui.add_space(2.0);
        ui.label(RichText::new(heure).size(11.0).color(encre));
    }
    ui.add_space(8.0);
}

fn pieces_jointes(ui: &mut egui::Ui, pieces: &[&str], encre: Color32, opacite: f32) {
    for p in pieces {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(super::icone(icones::PIECE, 11.0).color(encre.gamma_multiply(opacite)));
            ui.label(RichText::new(*p).size(12.0).color(encre.gamma_multiply(opacite)));
        });
    }
}

/// Eve réfléchit : sa petite orbe, et trois points qui s'allument tour à tour dans une bulle blanche (celle qui attend
/// la réponse de l'assistant de l'ERP).
pub(super) fn points_de_suspension(ui: &mut egui::Ui) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        avatar(ui, false, 1.0);
        let fond = ui.painter().add(Shape::Noop);
        let r = egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 13))
            .show(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 8.0), Sense::hover());
                let t = ui.input(|i| i.time) as f32;
                for k in 0..3 {
                    let a = 0.3 + 0.7 * ((t * 5.0 - k as f32 * 0.8).sin() * 0.5 + 0.5);
                    let centre = pos2(r.left() + 5.0 + k as f32 * 12.0, r.center().y);
                    ui.painter().circle_filled(centre, 3.5, erp::TEXTE_3.gamma_multiply(a));
                }
            })
            .response
            .rect;
        ui.painter().set(fond, fond_de_bulle(r, false, 1.0));
    });
    ui.add_space(10.0);
}

// ---- La fenêtre dans ce thème ----------------------------------------------------------------------------------

/// Ce que la bannière dit de la météo, comme le badge du tableau de bord de l'ERP (`DashboardWeatherBadge.tsx`,
/// l. 30-51, 163-180) : l'icône et sa teinte sur le bleu de la bannière (le soleil en jaune, la pluie et la neige en
/// bleu pâle, l'orage en orange, le reste en blanc), la température, le temps qu'il fait, la ville.
#[derive(Debug, PartialEq)]
struct MeteoDuJour {
    icone: char,
    teinte: Color32,
    temperature: String,
    temps: String,
    ville: String,
}

/// La météo du jour d'après la ligne du bulletin (« MONTRÉAL · 5 °C · COUVERT », ou sans la ville), son code WMO et le
/// nom de la ville tel que la famille l'écrit (vide : celui de la ligne).
fn meteo_du_jour(ligne: &str, code: u32, ville: &str) -> Option<MeteoDuJour> {
    let morceaux: Vec<&str> = ligne.split(" · ").map(str::trim).collect();
    let k = morceaux.iter().position(|m| m.contains('°'))?;
    // Une ville en capitales, « SAINT-JEAN » → « Saint-Jean ».
    let nom_propre = |m: &str| {
        let mut sortie = String::new();
        let mut debut = true;
        for c in m.chars() {
            if debut {
                sortie.extend(c.to_uppercase())
            } else {
                sortie.extend(c.to_lowercase())
            }
            debut = matches!(c, '-' | ' ' | '\'');
        }
        sortie
    };
    let ville = if ville.trim().is_empty() {
        morceaux[..k].first().map(|v| nom_propre(v)).unwrap_or_default()
    } else {
        ville.trim().to_string()
    };
    let (icone, teinte) = match code {
        0 | 1 => (pictos::SOLEIL, SOLEIL_DE_LA_BANNIERE),
        2 => (pictos::SOLEIL_VOILE, SOLEIL_DE_LA_BANNIERE),
        51..=67 | 71..=77 | 80..=86 => (pictos::NUAGE, Color32::from_rgb(0xBF, 0xDB, 0xFE)),
        95..=99 => (pictos::NUAGE, Color32::from_rgb(0xFD, 0xBA, 0x74)),
        _ => (pictos::NUAGE, Color32::WHITE),
    };
    Some(MeteoDuJour {
        icone,
        teinte,
        temperature: morceaux[k].to_string(),
        temps: morceaux.get(k + 1).map(|m| casse_de_phrase(m)).unwrap_or_default(),
        ville,
    })
}

/// Le jaune du soleil sur le bleu de la bannière.
const SOLEIL_DE_LA_BANNIERE: Color32 = Color32::from_rgb(0xFD, 0xE0, 0x47);

impl Fenetre {
    /// Le thème choisi (son nom, [`THEMES`]) : celui que publie le chef, ou celui qu'on vient de choisir d'un clic,
    /// tenu le temps que le chef le publie (pas de retour en arrière d'une image) ; un nom inconnu, le centre.
    pub(super) fn theme(&self, ctx: &egui::Context) -> &'static str {
        let maintenant = ctx.input(|i| i.time);
        let rang = match ctx.data(|d| d.get_temp::<(usize, f64)>(egui::Id::unique(THEME_TENU))) {
            Some((choisi, quand)) if maintenant - quand < TENUE => choisi,
            _ => THEMES.iter().position(|t| t.0 == self.champ("theme")).unwrap_or(0),
        };
        THEMES[rang.min(THEMES.len() - 1)].0
    }

    /// Le thème « Constructo AI » est choisi.
    pub(super) fn theme_constructo(&self, ctx: &egui::Context) -> bool {
        self.theme(ctx) == "constructo"
    }

    /// La rubrique « Thème » des réglages : le choix en segments (« Réseau neuronal », « Voie lactée », « Constructo
    /// AI ») ; le geste `theme` au chef, qui le retient dans `reglages.json`.
    pub(super) fn choix_du_theme(&mut self, ui: &mut egui::Ui) {
        let actuel = THEMES.iter().position(|t| t.0 == self.theme(ui.ctx())).unwrap_or(0);
        let mots: Vec<&str> = THEMES.iter().map(|t| t.1).collect();
        if let Some(k) = segments(ui, "theme", &mots, actuel)
            && k != actuel
        {
            let maintenant = ui.input(|i| i.time);
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(THEME_TENU), (k, maintenant)));
            self.geste("theme", Value::String(THEMES[k].0.into()));
        }
    }

    /// Le portrait d'Eve dans les réglages, sous le choix du thème « Constructo AI », le seul qui le montre : sa
    /// vignette (sinon l'orbe), « Choisir une image… » ou « Changer d'image… » (la boîte « Ouvrir » de Windows), et
    /// « Retirer ». L'image se copie dans le dossier local ([`portrait::poser`]), et la fenêtre la prend d'elle-même ;
    /// un refus va au fil, comme celui d'une pièce jointe.
    pub(super) fn choix_du_portrait(&mut self, ui: &mut egui::Ui) {
        if !self.theme_constructo(ui.ctx()) {
            return;
        }
        let visage = self.portrait.visage(ui.ctx());
        ui.add_space(8.0);
        super::groupe(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                match &visage {
                    Some(v) => v.vignette(ui.painter(), r.center(), 22.0),
                    None => sphere(ui.painter(), r.center(), 17.0),
                }
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new("Portrait d'Eve").size(14.0).color(couleurs::ENCRE));
                    let aide =
                        if visage.is_some() { "À la place de l'orbe" } else { "Une photo à la place de l'orbe" };
                    ui.label(RichText::new(aide).size(12.0).color(couleurs::ENCRE_3));
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let mot = if visage.is_some() { "Changer d'image…" } else { "Choisir une image…" };
                let rep = super::bouton(ui, None, mot, super::Bouton::Cerne);
                let aide = "Une photo, ou un PNG détouré (au fond transparent) : sa tête sort alors du cercle";
                if rep.on_hover_text(aide).clicked() {
                    let (local, rapport) = (self.portrait.local().to_path_buf(), self.rapport.clone());
                    let ctx = ui.ctx().clone();
                    pieces::choisir_puis(move |choisies| {
                        let refus = match choisies {
                            Ok(images) => images.first().and_then(|image| {
                                portrait::poser(image, &local)
                                    .err()
                                    .map(|e| format!("Le portrait d'Eve n'est pas posé : {e}"))
                            }),
                            Err(e) => Some(format!("La boîte de dialogue « Ouvrir » ne s'ouvre pas : {e}")),
                        };
                        if let Some(r) = refus {
                            let evt = crate::chef::Evt::Ui { action: "piece_erreur".into(), valeur: Value::String(r) };
                            let _ = rapport.send(evt);
                        }
                        ctx.request_repaint();
                    });
                }
                if visage.is_some() && super::bouton(ui, None, "Retirer", super::Bouton::Plat).clicked() {
                    self.portrait.retirer();
                }
            });
            ui.add_space(10.0);
        });
    }

    /// Le portrait à cette image, gardé pour les bulles et l'en-tête du dock ([`portrait_du_fil`]), qui se dessinent
    /// sans la fenêtre.
    fn garder_le_portrait(&mut self, ctx: &egui::Context) {
        let visage = self.portrait.visage(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(PORTRAIT), visage));
    }

    /// Le visage d'Eve dans `rect`, qui répond par `rep` : son portrait, s'il y en a un, son ombre dessous, cerné de
    /// l'anneau de ce qu'elle fait ([`portrait::anneau`], dans la couleur de la pastille de la bannière) — un portrait
    /// détouré passe devant l'anneau, la tête hors du cercle ; sinon l'orbe. Tenu, il parle, comme la touche.
    fn visage_dans(&mut self, ui: &mut egui::Ui, rect: Rect, rep: egui::Response) {
        let Some(visage) = self.portrait.visage(ui.ctx()) else {
            return self.orbe_dans(ui, rect, rep);
        };
        self.orbe_tenue = rep.is_pointer_button_down_on();
        rep.on_hover_text("Tiens appuyé pour parler").on_hover_cursor(egui::CursorIcon::PointingHand);
        let etat = self.champ("etat").to_string();
        // Le niveau qui anime l'anneau : sa voix quand elle parle (lue à chaque image, comme sa bouche), celui du micro
        // sinon.
        let parle = etat == "parole";
        let niveau = if parle {
            crate::voix::bouche_du_moment().map_or(0.0, |(n, _)| n)
        } else {
            self.etat.get("niveau").and_then(Value::as_f64).unwrap_or(0.0) as f32
        };
        let temps = ui.input(|i| i.time);
        let vivant = animations(ui.ctx());
        // L'émotion de sa réponse ; pendant qu'elle réfléchit, son air pensif.
        let emotion = if etat == "reflexion" {
            Some(Emotion::Reflexion)
        } else {
            self.etat.get("emotion").and_then(Value::as_str).and_then(Emotion::de)
        };
        let pose = self.portrait.pose_du_moment(temps, parle, vivant, emotion).unwrap_or_default();
        let (centre, rayon) = (rect.center(), rect.width().min(rect.height()) * 0.4);
        let couleur = couleur_de_l_anneau(&etat, ui.visuals().dark_mode);
        let p = ui.painter();
        p.circle_filled(centre + vec2(0.0, rayon * 0.08), rayon * 1.06, Color32::from_black_alpha(16));
        p.circle_filled(centre + vec2(0.0, rayon * 0.03), rayon * 1.01, Color32::from_black_alpha(34));
        portrait::anneau(p, centre, rayon, &etat, couleur, niveau, temps);
        visage.en_grand(p, centre, rayon, &pose);
        portrait::pastille(p, centre, rayon, &etat, couleur);
        // Redessinée tant qu'elle s'anime (elle respire) ; sans les animations de Windows, au prochain clignement.
        if etat != "pret" || pose.apres <= 0.0 {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(pose.apres));
        }
    }

    /// La fenêtre sous la barre, dans le thème « Constructo AI » : le fond qui respire ; le centre déplié, le tableau
    /// de bord, et la conversation à droite comme le dock de l'assistant de l'ERP ; replié (ou la fenêtre étroite), la
    /// conversation seule, son orbe en tête.
    pub(super) fn constructo(&mut self, ui: &mut egui::Ui) {
        self.garder_le_portrait(ui.ctx());
        let zone = ui.available_rect_before_wrap();
        let anime = animations(ui.ctx());
        let fond = Fond { zone, course: if anime { course(ui.input(|i| i.time)) } else { 0.5 } };
        ui.painter().add(Shape::mesh(fond.maillage()));
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(FOND), fond));
        // Le fond respire : la fenêtre se redessine comme quand le réseau s'anime.
        self.anime = anime;
        visuels_clairs(ui.visuals_mut());
        if self.centre_ouvert && self.large {
            egui::Panel::right("conversation-constructo")
                .resizable(true)
                .default_size(440.0)
                .size_range(360.0..=720.0)
                .show_separator_line(false)
                .frame(egui::Frame::new())
                .show(ui, |ui| self.dock(ui));
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.tableau_de_bord(ui));
        } else {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
                // Seule, la conversation garde une largeur de lecture, au milieu.
                let r = ui.max_rect();
                let colonne = Rect::from_center_size(r.center(), vec2(r.width().min(820.0), r.height()));
                let colonne = colonne.shrink2(vec2(16.0, 0.0));
                ui.scope_builder(UiBuilder::new().max_rect(colonne), |ui| self.conversation_seule(ui));
            });
        }
    }

    /// La conversation à droite, comme le dock de l'assistant de l'ERP (`AssistantDock.tsx`) : un filet à gauche, son
    /// en-tête marine, la demande d'accord, le fil sur le fond, la saisie.
    fn dock(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        ui.painter().vline(r.left() + 0.5, r.y_range(), Stroke::new(1.0, erp::FILET));
        egui::Panel::top("tete-du-dock")
            .exact_size(54.0)
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.tete_du_dock(ui));
        if let Some(a) = self.etat.get("accord").filter(|a| !a.is_null()).cloned() {
            egui::Panel::top("accord-du-dock")
                .show_separator_line(false)
                .frame(egui::Frame::new().inner_margin(Margin { left: 14, right: 12, top: 12, bottom: 0 }))
                .show(ui, |ui| self.accord_clair(ui, &a));
        }
        egui::Panel::bottom("saisie-du-dock")
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.saisie_claire(ui, false));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin { left: 14, right: 4, top: 6, bottom: 0 }))
            .show(ui, |ui| self.fil(ui, Style::Claire));
    }

    /// L'en-tête du dock (`.erp-topbar`, comme celui de l'ERP) : la tuile de la petite orbe, « Conversation » et ce que
    /// fait Eve, et, à droite, « nouvelle conversation » et « arrêter ».
    fn tete_du_dock(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        degrade(ui.painter(), rect, erp::MARINE_HAUT, erp::MARINE);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, Color32::from_black_alpha(51)));
        ui.horizontal_centered(|ui| {
            ui.add_space(14.0);
            let (tuile, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            if let Some(v) = portrait_du_fil(ui.ctx()) {
                v.vignette(ui.painter(), tuile.center(), 15.0);
                ui.painter().circle_stroke(tuile.center(), 15.0, Stroke::new(1.0, Color32::from_white_alpha(90)));
            } else {
                ui.painter().rect_filled(tuile, 4.0, Color32::from_white_alpha(38));
                sphere(ui.painter(), tuile.center(), 10.0);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let titre = RichText::new("Conversation").family(FontFamily::Name(GRAS.into())).size(14.0);
                ui.label(titre.color(Color32::WHITE));
                let libelle = self.champ("libelle");
                let sous_titre = if libelle.is_empty() { "Eve" } else { libelle };
                ui.add(
                    egui::Label::new(RichText::new(sous_titre).size(11.5).color(Color32::from_white_alpha(178)))
                        .truncate(),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                if bouton_d_en_tete(ui, pictos::TAIRE).on_hover_text("Arrêter : Eve se tait").clicked() {
                    self.geste("arreter", Value::Null);
                }
                if bouton_d_en_tete(ui, icones::NOUVELLE).on_hover_text("Nouvelle conversation").clicked() {
                    self.geste("nouvelle", Value::Null);
                }
            });
        });
    }

    /// La demande d'accord, dans le cadre des avertissements de l'ERP (`.erp-alert-warning`) : ce qui va partir, et les
    /// deux boutons, pour la demande affichée seulement (son numéro).
    fn accord_clair(&self, ui: &mut egui::Ui, a: &Value) {
        let numero = a.get("numero").cloned().unwrap_or(Value::Null);
        let texte = |cle: &str| a.get(cle).and_then(Value::as_str).unwrap_or("").to_string();
        egui::Frame::new()
            .fill(erp::AVERTISSEMENT_FOND)
            .stroke((1.0, erp::AVERTISSEMENT))
            .corner_radius(4)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.label(super::icone(icones::ALERTE, 15.0).color(erp::AVERTISSEMENT));
                    ui.label(semi("J'ai besoin de ton accord", 14.0).color(erp::AVERTISSEMENT_ENCRE));
                });
                ui.label(RichText::new(texte("quoi")).size(14.0).color(erp::TEXTE));
                let detail = texte("detail");
                if !detail.is_empty() {
                    egui::Frame::new()
                        .fill(Color32::from_white_alpha(200))
                        .stroke((1.0, erp::AVERTISSEMENT_BORD))
                        .corner_radius(4)
                        .inner_margin(Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                                ui.label(RichText::new(detail).monospace().size(12.0).color(erp::TEXTE_2));
                            });
                        });
                }
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if bouton_erp(ui, Some(icones::OUI), "Je confirme", BoutonErp::Oui).clicked() {
                        self.geste("confirmer", numero.clone());
                    }
                    if bouton_erp(ui, Some(icones::NON), "Annuler", BoutonErp::Secondaire).clicked() {
                        self.geste("annuler", numero.clone());
                    }
                    if let Some(n) = a.get("restants").and_then(Value::as_u64).filter(|n| *n > 0) {
                        ui.label(RichText::new(format!("encore {n} après celle-ci")).size(12.0).color(erp::TEXTE_2));
                    }
                });
            });
        ui.add_space(10.0);
    }

    /// La saisie, comme celle de l'assistant de l'ERP (`AssistantIAChat.tsx`, l. 1156-1233) : un filet au-dessus ; les
    /// pièces en attente ; le micro (à tenir pour parler), le trombone (ses boutons d'outil), le champ (`.erp-input`),
    /// l'envoi (le bouton primaire). `avec_actions` : dessous, arrêter et recommencer (sans le dock, qui les porte dans
    /// son en-tête).
    fn saisie_claire(&mut self, ui: &mut egui::Ui, avec_actions: bool) {
        let r = ui.max_rect();
        ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, erp::FILET));
        ui.add_space(12.0);
        self.pieces_en_attente(ui, &pieces::CLAIR, 12.0);
        let id = egui::Id::unique("saisie-texte-constructo");
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            self.bouton_micro_clair(ui);
            self.trombone(ui, &pieces::CLAIR);
            let actif = ui.ctx().memory(|m| m.has_focus(id));
            let largeur = (ui.available_width() - 40.0 - 8.0 - 12.0).max(60.0);
            let champ = egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(if actif { Stroke::new(2.0, erp::PRIMAIRE) } else { Stroke::new(1.0, erp::FILET_CHAMP) })
                .corner_radius(4)
                .inner_margin(if actif { Margin::same(0) } else { Margin::same(1) })
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.saisie)
                            .id(id)
                            .frame(egui::Frame::new().inner_margin(Margin::symmetric(11, 10)))
                            .font(FontId::proportional(14.0))
                            .text_color(erp::TEXTE)
                            .hint_text(RichText::new("Écris ta demande à Eve…").color(erp::TEXTE_3))
                            .desired_width(largeur - 2.0),
                    )
                })
                .inner;
            if std::mem::take(&mut self.saisie_au_clavier) {
                champ.request_focus();
            }
            let entree = champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let pret = !self.saisie.trim().is_empty();
            if (bouton_envoyer_clair(ui, pret).clicked() || entree) && pret {
                let t = std::mem::take(&mut self.saisie).trim().to_string();
                self.geste("texte", Value::String(t));
                champ.request_focus();
            }
        });
        if avec_actions {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                let arreter = bouton_erp(ui, Some(pictos::TAIRE), "Arrêter", BoutonErp::Fantome);
                if arreter.on_hover_text("Eve se tait").clicked() {
                    self.geste("arreter", Value::Null);
                }
                if bouton_erp(ui, Some(icones::NOUVELLE), "Nouvelle conversation", BoutonErp::Fantome).clicked() {
                    self.geste("nouvelle", Value::Null);
                }
            });
        }
        ui.add_space(12.0);
    }

    /// Le micro, à tenir comme la touche (ou l'orbe) : un bouton d'outil de l'ERP (blanc, son filet gris) ; vert, et
    /// une onde qui s'en échappe, tant qu'Eve écoute.
    fn bouton_micro_clair(&mut self, ui: &mut egui::Ui) {
        let (rect, rep) = ui.allocate_exact_size(vec2(42.0, 42.0), Sense::click());
        self.tenir_pour_parler(rep.is_pointer_button_down_on() || self.orbe_tenue);
        let ecoute = self.parle || self.champ("etat") == "ecoute";
        let survol = rep.hovered();
        let (fond, bord, encre) = match (ecoute, survol) {
            (true, _) => (erp::SUCCES_FOND, erp::SUCCES, erp::SUCCES_ENCRE),
            (false, true) => (Color32::WHITE, erp::TEXTE_3, erp::TEXTE),
            (false, false) => (Color32::from_white_alpha(190), erp::FILET_BOUTON, erp::TEXTE_2),
        };
        let p = ui.painter();
        if ecoute {
            let f = (ui.input(|i| i.time) as f32 * 1.6).fract();
            let onde = rect.expand(f * 6.0);
            p.rect_stroke(
                onde,
                4.0 + f * 6.0,
                Stroke::new(2.0, erp::SUCCES.gamma_multiply(1.0 - f)),
                StrokeKind::Outside,
            );
        }
        p.rect(rect, 4.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
        let police = FontId::new(17.0, FontFamily::Name(ICONES.into()));
        p.text(rect.center(), Align2::CENTER_CENTER, icones::MICRO, police, encre);
        rep.on_hover_text("Maintiens pour parler").on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    /// Le haut de la conversation seule : l'orbe (ou le portrait d'Eve), ce qu'Eve fait (comme l'Eve de Constructo AI
    /// dans son thème clair : 19 points en gras), comment lui parler ; la demande d'accord, s'il y en a une.
    fn tete_claire(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            let cote = self.cote_de_l_orbe(ui.ctx());
            let (rect, rep) = ui.allocate_exact_size(vec2(cote, cote), Sense::click());
            self.visage_dans(ui, rect, rep);
            let etat = RichText::new(self.champ("libelle")).family(FontFamily::Name(GRAS.into())).size(19.0);
            ui.label(etat.color(erp::TEXTE));
            let aide = self.champ("aide");
            if !aide.is_empty() {
                ui.label(RichText::new(aide).size(13.0).color(erp::TEXTE_2));
            }
            ui.add_space(12.0);
        });
        if let Some(a) = self.etat.get("accord").filter(|a| !a.is_null()) {
            self.accord_clair(ui, a);
        }
    }

    /// La conversation seule (le centre replié, ou la fenêtre étroite) : sa tête, le fil, la saisie et ses actions.
    fn conversation_seule(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("tete-constructo")
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.tete_claire(ui));
        egui::Panel::bottom("saisie-constructo")
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| self.saisie_claire(ui, true));
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.fil(ui, Style::Claire));
    }

    /// Le tableau de bord, comme les beaux tableaux de bord d'aujourd'hui : une page qui défile — la bannière
    /// d'accueil ([`Fenetre::banniere`]), la décision du jour, puis les échéances à gauche et tout le panneau
    /// d'information en cartes à droite ; dans une place étroite, ou sans échéances, les cartes prennent toute la
    /// largeur.
    fn tableau_de_bord(&mut self, ui: &mut egui::Ui) {
        // La page défile sur toute la zone ; ses marges (celles de l'ERP, `p-6`) sont dedans, pour que les ombres des
        // cartes ne s'y coupent pas.
        let zone = ui.max_rect();
        let aujourdhui = journal::date();
        let horodatage = journal::horodatage();
        let heure = horodatage.get(11..16).unwrap_or("").replace('-', ":");
        let sortie =
            egui::ScrollArea::vertical().id_salt("tableau-de-bord").auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: 24, right: 24, top: 22, bottom: 30 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.banniere(ui, &aujourdhui, &horodatage);
                    ui.add_space(ECART);
                    if self.centre.decision_carte(ui, &aujourdhui) {
                        ui.add_space(ECART);
                    }
                    let largeur = ui.available_width();
                    let echeances = self.centre.a_des_echeances(&aujourdhui);
                    if largeur >= 900.0 && echeances {
                        // Deux colonnes : les échéances, les cartes ; la page descend jusqu'à la plus longue.
                        let gauche = (largeur * 0.38).clamp(330.0, 520.0);
                        let haut = ui.cursor().min;
                        let colonne = |ui: &mut egui::Ui, x: f32, l: f32| {
                            let place = Rect::from_min_size(pos2(x, haut.y), vec2(l, 100_000.0));
                            ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)))
                        };
                        let mut ug = colonne(ui, haut.x, gauche);
                        self.centre.echeances_carte(&mut ug, &aujourdhui);
                        let reste = largeur - gauche - ECART;
                        let mut ud = colonne(ui, haut.x + gauche + ECART, reste);
                        self.centre.panneau_cartes(&mut ud, &aujourdhui, &heure, colonnes_pour(reste));
                        ui.advance_cursor_after_rect(ug.min_rect().union(ud.min_rect()));
                    } else {
                        if echeances {
                            self.centre.echeances_carte(ui, &aujourdhui);
                            ui.add_space(ECART);
                        }
                        self.centre.panneau_cartes(ui, &aujourdhui, &heure, colonnes_pour(largeur));
                    }
                });
            });
        // Défilée, la page s'estompe sous la barre au lieu d'y être coupée net.
        if sortie.state.clamped_offset().y > 1.0 {
            fondu(ui.painter(), Rect::from_min_size(zone.min, vec2(zone.width(), 24.0)));
        }
    }

    /// La bannière d'accueil, en tête du tableau de bord : un dégradé du marine de Constructo AI au bleu du ciel, deux
    /// anneaux et deux lueurs ; l'orbe d'Eve à gauche (tenue, elle parle) ; « Tableau de bord », le salut du moment et
    /// le prénom de la personne (« Bonsoir, Sam »), la date et l'heure, et ce que fait Eve dans une pastille (son
    /// point de couleur, qui bat quand elle travaille, et comment lui parler) ; à droite, la météo en grand (sans le
    /// temps qu'il fait dans une bannière étroite, et pas du tout dans une très étroite).
    fn banniere(&mut self, ui: &mut egui::Ui, aujourdhui: &str, horodatage: &str) {
        let hauteur = (self.hauteur * 0.2).clamp(160.0, 196.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), hauteur), Sense::hover());
        let texture = texture_de_la_banniere(ui.ctx(), rect.size());
        let p = ui.painter();
        p.add(ombre_marine(rect, RAYON_DE_CARTE, 10, 30, 64));
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        p.add(egui::epaint::RectShape::filled(rect, RAYON_DE_CARTE, Color32::WHITE).with_texture(texture.id(), uv));
        let cote = hauteur - 24.0;
        let orbe = Rect::from_min_size(pos2(rect.left() + 20.0, rect.center().y - cote / 2.0), vec2(cote, cote));
        // Les ornements, gardés dans la bannière : deux anneaux à droite, une lueur derrière l'orbe, une autre à droite.
        let ornements = p.with_clip_rect(rect.shrink(1.0));
        let anneaux = pos2(rect.right() - hauteur * 0.55, rect.top() + hauteur * 0.15);
        for (r, a) in [(hauteur * 0.95, 22), (hauteur * 1.45, 14)] {
            ornements.circle_stroke(anneaux, r, Stroke::new(1.5, Color32::from_white_alpha(a)));
        }
        lueur(&ornements, orbe.center(), cote * 0.75, 0.16);
        lueur(&ornements, pos2(rect.right() - hauteur * 0.7, rect.center().y), hauteur * 0.8, 0.07);
        // L'orbe — ou le portrait d'Eve — : dans ses couleurs du centre de commande, faites pour un fond sombre.
        let rep = ui.interact(orbe, egui::Id::unique("orbe-de-la-banniere"), Sense::click());
        ui.scope(|ui| {
            ui.visuals_mut().dark_mode = true;
            self.visage_dans(ui, orbe, rep);
        });
        let p = ui.painter();
        let x = orbe.right() + 24.0;
        // La météo, contre le bord droit, si le texte garde sa place ; le texte prend le reste.
        let ville = self.meteo.as_ref().map(|m| m.lieu().ville.clone()).unwrap_or_default();
        let jour = self.bulletin.as_ref().and_then(|b| meteo_du_jour(&b.ligne, b.code, &ville));
        let meteo = jour.and_then(|m| {
            [false, true].into_iter().map(|compacte| MeteoDeLaBanniere::nouvelle(p, &m, compacte)).find(|g| {
                let reste = rect.right() - 30.0 - g.largeur() - 24.0 - x;
                reste >= if g.compacte { 200.0 } else { 280.0 }
            })
        });
        let droite = meteo.as_ref().map_or(rect.right() - 28.0, |g| rect.right() - 30.0 - g.largeur() - 24.0);
        let l = (droite - x).max(120.0);
        // Le texte, de haut en bas.
        let mut surtitre = egui::text::LayoutJob::default();
        let semi_11 = FontId::new(11.0, FontFamily::Name(SEMI.into()));
        let format =
            egui::text::TextFormat { font_id: semi_11, color: Color32::from_white_alpha(170), ..Default::default() };
        surtitre.append("TABLEAU DE BORD", 0.0, egui::text::TextFormat { extra_letter_spacing: 1.6, ..format });
        let g_surtitre = p.layout_job(surtitre);
        let qui = self.champ("qui");
        let salut = salut_du_moment(horodatage.get(11..13).and_then(|h| h.parse().ok()).unwrap_or(12));
        let salut = if qui.is_empty() { salut.to_string() } else { format!("{salut}, {qui}") };
        let gras = |taille: f32| FontId::new(taille, FontFamily::Name(GRAS.into()));
        let large = p.layout_no_wrap(salut.clone(), gras(30.0), Color32::WHITE).size().x;
        let taille = if large > l { (30.0 * l / large).max(22.0) } else { 30.0 };
        let g_salut = p.layout(salut, gras(taille), Color32::WHITE, l);
        let date = casse_de_phrase(&ambiance::date_longue(aujourdhui));
        let date = format!("{date} · {}", ambiance::heure_de_veille(horodatage));
        let g_date = p.layout_no_wrap(date, FontId::proportional(15.0), Color32::from_white_alpha(220));
        let g_etat = ligne_de_l_etat(p, self.champ("libelle"), self.champ("aide"), l - 44.0);
        let h_pilule = g_etat.size().y + 12.0;
        let tailles = [g_surtitre.size().y, g_salut.size().y, g_date.size().y];
        let total = tailles.iter().sum::<f32>() + 6.0 + 2.0 + 14.0 + h_pilule;
        let mut y = rect.center().y - total / 2.0;
        for (g, apres) in [(g_surtitre, 6.0), (g_salut, 2.0), (g_date, 14.0)] {
            let h = g.size().y;
            p.galley(pos2(x, y), g, Color32::WHITE);
            y += h + apres;
        }
        let pilule = Rect::from_min_size(pos2(x, y), vec2(g_etat.size().x + 44.0, h_pilule));
        let (fond, filet) = (Color32::from_white_alpha(28), Color32::from_white_alpha(46));
        p.rect(pilule, 255.0, fond, Stroke::new(1.0, filet), StrokeKind::Inside);
        let etat = self.champ("etat");
        let point = pos2(pilule.left() + 17.0, pilule.center().y);
        let couleur = couleur_de_l_etat(etat);
        if !matches!(etat, "pret" | "demarrage" | "erreur") {
            // Eve travaille : son point bat.
            let f = (ui.input(|i| i.time) * 1.2).fract() as f32;
            p.circle_filled(point, 4.0 + 6.0 * f, couleur.gamma_multiply(0.45 * (1.0 - f)));
        }
        p.circle_filled(point, 7.0, couleur.gamma_multiply(0.28));
        p.circle_filled(point, 4.0, couleur);
        p.galley(pos2(pilule.left() + 30.0, pilule.center().y - g_etat.size().y / 2.0), g_etat, Color32::WHITE);
        if let Some(g) = meteo {
            g.peindre(p, rect.right() - 30.0, rect.center().y);
        }
    }
}

/// La météo de la bannière, mise en page : son icône en grand, la température en très grand, et dessous le temps qu'il
/// fait et la ville (pas dans une bannière étroite : `compacte`).
struct MeteoDeLaBanniere {
    icone: std::sync::Arc<egui::Galley>,
    temperature: std::sync::Arc<egui::Galley>,
    dessous: Option<std::sync::Arc<egui::Galley>>,
    compacte: bool,
}

impl MeteoDeLaBanniere {
    fn nouvelle(p: &egui::Painter, m: &MeteoDuJour, compacte: bool) -> Self {
        let icone = p.layout_no_wrap(m.icone.to_string(), FontId::new(40.0, FontFamily::Name(ICONES.into())), m.teinte);
        let gras = FontId::new(36.0, FontFamily::Name(GRAS.into()));
        let temperature = p.layout_no_wrap(m.temperature.clone(), gras, Color32::WHITE);
        let mots: Vec<&str> = [m.temps.as_str(), m.ville.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
        let dessous = (!compacte && !mots.is_empty())
            .then(|| p.layout_no_wrap(mots.join(" · "), FontId::proportional(14.0), Color32::from_white_alpha(210)));
        Self { icone, temperature, dessous, compacte }
    }

    fn largeur(&self) -> f32 {
        let texte = self.dessous.as_ref().map_or(0.0, |g| g.size().x).max(self.temperature.size().x);
        self.icone.size().x + 14.0 + texte
    }

    /// Peinte contre `droite`, centrée sur `milieu`.
    fn peindre(self, p: &egui::Painter, droite: f32, milieu: f32) {
        let gauche = droite - self.largeur();
        let h_icone = self.icone.size().y;
        let l_icone = self.icone.size().x;
        p.galley(pos2(gauche, milieu - h_icone / 2.0), self.icone, Color32::WHITE);
        let x = gauche + l_icone + 14.0;
        let h_temperature = self.temperature.size().y;
        let h = h_temperature + self.dessous.as_ref().map_or(0.0, |g| g.size().y);
        p.galley(pos2(x, milieu - h / 2.0), self.temperature, Color32::WHITE);
        if let Some(g) = self.dessous {
            p.galley(pos2(x, milieu - h / 2.0 + h_temperature), g, Color32::WHITE);
        }
    }
}

/// Le dégradé de la bannière en texture, à un texel pour quatre points (le filtrage linéaire le lisse) : egui en adoucit
/// les coins arrondis, ce qu'un maillage ne fait pas. Refaite quand la bannière change de taille.
fn texture_de_la_banniere(ctx: &egui::Context, taille: egui::Vec2) -> egui::TextureHandle {
    let id = egui::Id::unique("texture-de-la-banniere");
    let cle = [(taille.x / 4.0).round().max(2.0) as usize, (taille.y / 4.0).round().max(2.0) as usize];
    if let Some((c, t)) = ctx.data(|d| d.get_temp::<([usize; 2], egui::TextureHandle)>(id))
        && c == cle
    {
        return t;
    }
    let r = Rect::from_min_size(Pos2::ZERO, vec2(cle[0] as f32, cle[1] as f32));
    let arrets = [erp::MARINE_HAUT, erp::AZUR, erp::CIEL];
    let pixels = (0..cle[0] * cle[1])
        .map(|i| degrade_css(r, 110.0, &arrets, pos2((i % cle[0]) as f32 + 0.5, (i / cle[0]) as f32 + 0.5)))
        .collect();
    let t = ctx.load_texture("banniere", egui::ColorImage::new(cle, pixels), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (cle, t.clone())));
    t
}

/// Une lueur blanche et ronde, qui s'efface de son centre (à `force`) à son bord sans anneau visible : trois couronnes
/// d'un maillage, de plus en plus pâles.
fn lueur(p: &egui::Painter, centre: Pos2, rayon: f32, force: f32) {
    const COURONNES: [(f32, f32); 4] = [(0.0, 1.0), (0.35, 0.6), (0.7, 0.22), (1.0, 0.0)];
    let n = 48;
    let mut m = Mesh::default();
    m.colored_vertex(centre, Color32::from_white_alpha((255.0 * force) as u8));
    for &(r, a) in &COURONNES[1..] {
        let couleur = Color32::from_white_alpha((255.0 * force * a) as u8);
        for i in 0..n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            m.colored_vertex(centre + vec2(t.cos(), t.sin()) * rayon * r, couleur);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_triangle(0, 1 + i, 1 + j);
        for c in 0..COURONNES.len() as u32 - 2 {
            let (a, b) = (1 + c * n, 1 + (c + 1) * n);
            m.add_triangle(a + i, b + i, b + j);
            m.add_triangle(a + i, b + j, a + j);
        }
    }
    p.add(Shape::mesh(m));
}

/// La ligne de la pastille de la bannière : ce que fait Eve en demi-gras, puis comment lui parler, ses touches (entre
/// crochets dans l'aide du chef) en demi-gras aussi ; coupée d'une ellipse à `largeur`.
fn ligne_de_l_etat(p: &egui::Painter, libelle: &str, aide: &str, largeur: f32) -> std::sync::Arc<egui::Galley> {
    let semi = egui::text::TextFormat::simple(FontId::new(13.0, FontFamily::Name(SEMI.into())), Color32::WHITE);
    let normal = egui::text::TextFormat::simple(FontId::proportional(13.0), Color32::from_white_alpha(215));
    let mut job = egui::text::LayoutJob::default();
    job.append(libelle, 0.0, semi.clone());
    if !aide.is_empty() {
        job.append("  ·  ", 0.0, normal.clone());
        // « Maintiens [Ctrl+Espace] pour parler » : le texte, la touche, le texte…
        for (k, morceau) in aide.split(['[', ']']).enumerate() {
            job.append(morceau, 0.0, if k % 2 == 1 { semi.clone() } else { normal.clone() });
        }
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(largeur.max(60.0));
    p.layout_job(job)
}

/// Le salut du moment : bonjour le matin, bon après-midi, bonsoir, bonne nuit de 23 h à 5 h.
fn salut_du_moment(heure: u32) -> &'static str {
    match heure {
        5..=11 => "Bonjour",
        12..=17 => "Bon après-midi",
        18..=22 => "Bonsoir",
        _ => "Bonne nuit",
    }
}

/// Le point de couleur de ce que fait Eve, dans la pastille de la bannière : vert, prête ; corail, elle écoute ; ciel,
/// elle transcrit ; pervenche, elle réfléchit ; bleu, elle parle ; ambre, elle attend un accord ; rouge, un problème ;
/// gris, elle se prépare.
fn couleur_de_l_etat(etat: &str) -> Color32 {
    match etat {
        "pret" => Color32::from_rgb(0x4A, 0xDE, 0x80),
        "ecoute" => Color32::from_rgb(0xFF, 0x8A, 0x65),
        "transcription" => Color32::from_rgb(0x7D, 0xD3, 0xFC),
        "reflexion" => Color32::from_rgb(0xA5, 0xB4, 0xFC),
        "parole" => Color32::from_rgb(0x93, 0xC5, 0xFD),
        "confirmation" => Color32::from_rgb(0xFB, 0xBF, 0x24),
        "erreur" => Color32::from_rgb(0xF8, 0x71, 0x71),
        _ => Color32::from_rgb(0xCB, 0xD5, 0xE1),
    }
}

/// La couleur de l'anneau du portrait d'Eve : sur le fond sombre de la bannière, celle de la pastille ; sur un fond
/// clair (la conversation seule), où les pastels s'effacent, la même teinte, franche.
fn couleur_de_l_anneau(etat: &str, sombre: bool) -> Color32 {
    if sombre {
        return couleur_de_l_etat(etat);
    }
    match etat {
        "pret" => erp::SUCCES,
        "ecoute" => Color32::from_rgb(0xF9, 0x73, 0x16),
        "transcription" => erp::CIEL,
        "reflexion" => Color32::from_rgb(0x63, 0x66, 0xF1),
        "parole" => erp::PRIMAIRE,
        "confirmation" => erp::AVERTISSEMENT,
        "erreur" => Color32::from_rgb(0xDC, 0x26, 0x26),
        _ => Color32::from_rgb(0x94, 0xA3, 0xB8),
    }
}

/// Un bouton de l'en-tête du dock : une icône blanche à 70 %, un fond blanc à 10 % au survol (`hover:bg-white/10`).
fn bouton_d_en_tete(ui: &mut egui::Ui, code: char) -> egui::Response {
    let (rect, rep) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let survol = rep.hovered();
    let p = ui.painter();
    if survol {
        p.rect_filled(rect, 4.0, Color32::from_white_alpha(26));
    }
    let encre = if survol { Color32::WHITE } else { Color32::from_white_alpha(178) };
    p.text(rect.center(), Align2::CENTER_CENTER, code, FontId::new(15.0, FontFamily::Name(ICONES.into())), encre);
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// L'envoi : le bouton primaire de l'ERP (#0078D4, #005EA2 au survol), à moitié effacé quand il n'y a rien à envoyer.
fn bouton_envoyer_clair(ui: &mut egui::Ui, pret: bool) -> egui::Response {
    let (rect, rep) = ui.allocate_exact_size(vec2(42.0, 42.0), Sense::click());
    let fond = match (pret, rep.hovered()) {
        (false, _) => melange(erp::PRIMAIRE, Color32::WHITE, 0.5),
        (true, true) => erp::PRIMAIRE_FONCE,
        (true, false) => erp::PRIMAIRE,
    };
    let p = ui.painter();
    p.rect_filled(rect, 4.0, fond);
    let police = FontId::new(16.0, FontFamily::Name(ICONES.into()));
    p.text(rect.center(), Align2::CENTER_CENTER, icones::ENVOYER, police, Color32::WHITE);
    rep.on_hover_text("Envoyer (Entrée)").on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Le contexte de la fenêtre ouverte : un essai la capture par lui (`eve --essai-theme`).
pub(crate) fn contexte() -> Option<egui::Context> {
    super::CONTEXTE.get().cloned()
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn la_courbe_ease_est_celle_de_css() {
        assert!(ease(0.0).abs() < 1e-9);
        assert!((ease(1.0) - 1.0).abs() < 1e-9);
        // cubic-bezier(0.25, 0.1, 0.25, 1) à la moitié du temps : 80,24 % de la course (la valeur des navigateurs).
        assert!((ease(0.5) - 0.8024).abs() < 1e-3, "{}", ease(0.5));
        let mut avant = 0.0;
        for k in 1..=100 {
            let v = ease(f64::from(k) / 100.0);
            assert!(v >= avant, "la courbe monte toujours");
            avant = v;
        }
    }

    #[test]
    fn le_fond_respire_en_vingt_secondes() {
        assert!(course(0.0).abs() < 1e-6, "il part de 0 %");
        assert!((course(10.0) - 1.0).abs() < 1e-6, "à 10 s, il est à 100 %");
        assert!(course(20.0).abs() < 1e-6, "et il revient en 20 s");
        assert!((course(5.0) - 0.8024).abs() < 1e-3, "l'aller suit la courbe ease");
        assert!((course(15.0) - (1.0 - 0.8024)).abs() < 1e-3, "le retour aussi");
        assert!((course(27.5) - course(7.5)).abs() < 1e-6);
    }

    #[test]
    fn le_degrade_suit_celui_de_l_erp() {
        // Une zone carrée : à 0 % de sa course, le coin en haut à gauche est à t = 0,8125, celui en bas à droite à
        // 0,5625 (un quart du dégradé se voit) ; à 100 %, de 0,4375 à 0,1875.
        let zone = Rect::from_min_size(pos2(10.0, 20.0), vec2(400.0, 400.0));
        let a = Fond { zone, course: 0.0 };
        assert!((a.t(zone.left_top()) - 0.8125).abs() < 1e-6);
        assert!((a.t(zone.right_bottom()) - 0.5625).abs() < 1e-6);
        let b = Fond { zone, course: 1.0 };
        assert!((b.t(zone.left_top()) - 0.4375).abs() < 1e-6);
        assert!((b.t(zone.right_bottom()) - 0.1875).abs() < 1e-6);
        // Les arrêts : le blanc, --ui-bg-1, le blanc, --ui-bg-2… et entre deux, le mélange.
        assert_eq!(couleur_du_degrade(0.0), Color32::WHITE);
        assert_eq!(couleur_du_degrade(0.2), erp::FOND_1);
        assert_eq!(couleur_du_degrade(0.6), erp::FOND_2);
        assert_eq!(couleur_du_degrade(1.0), erp::FOND_1);
        assert_eq!(couleur_du_degrade(0.1), melange(Color32::WHITE, erp::FOND_1, 0.5));
        // Le maillage couvre la zone (l'aire de ses triangles), et chaque sommet porte la couleur de sa place.
        let large = Rect::from_min_size(pos2(0.0, 58.0), vec2(1120.0, 830.0));
        for course in [0.0, 0.37, 0.5, 1.0] {
            let f = Fond { zone: large, course };
            let m = f.maillage();
            let aire: f32 = m
                .indices
                .chunks(3)
                .map(|t| {
                    let [a, b, c] = [0, 1, 2].map(|k| m.vertices[t[k] as usize].pos);
                    ((b - a).x * (c - a).y - (b - a).y * (c - a).x).abs() / 2.0
                })
                .sum();
            assert!((aire - large.area()).abs() < 1.0, "{course} : {aire} pour {}", large.area());
            assert!(m.vertices.iter().all(|v| v.color == f.couleur(v.pos)));
        }
    }

    #[test]
    fn une_couleur_se_lit_sur_le_blanc() {
        assert!((contraste(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert_eq!(lisible(erp::TEXTE), erp::TEXTE, "l'encre du texte se lit déjà");
        // Le cyan d'une personne, illisible sur le blanc, s'assombrit jusqu'à se lire.
        let cyan = Color32::from_rgb(0, 229, 255);
        assert!(contraste(cyan, Color32::WHITE) < 4.5);
        let c = lisible(cyan);
        assert!(contraste(c, Color32::WHITE) >= 4.5);
        assert!(c.b() > c.r(), "il garde sa teinte : {c:?}");
    }

    #[test]
    fn la_meteo_du_jour_comme_l_erp() {
        let m = meteo_du_jour("LAVAL · 3 °C · CIEL DÉGAGÉ", 0, "").unwrap();
        assert_eq!(
            m,
            MeteoDuJour {
                icone: pictos::SOLEIL,
                teinte: SOLEIL_DE_LA_BANNIERE,
                temperature: "3 °C".into(),
                temps: "Ciel dégagé".into(),
                ville: "Laval".into()
            }
        );
        let m = meteo_du_jour("SAINT-JEAN · -2 °C · NEIGE MODÉRÉE", 73, "Saint-Jean-sur-Richelieu").unwrap();
        assert_eq!((m.ville.as_str(), m.temps.as_str()), ("Saint-Jean-sur-Richelieu", "Neige modérée"));
        let m = meteo_du_jour("5 °C · COUVERT", 3, "").unwrap();
        assert_eq!((m.ville.as_str(), m.temperature.as_str(), m.teinte), ("", "5 °C", Color32::WHITE));
        assert_eq!(meteo_du_jour("", 0, ""), None);
    }

    #[test]
    fn les_textes_du_tableau_en_casse_de_phrase() {
        assert_eq!(casse_de_phrase("ÉPICERIE · 0 À ACHETER"), "Épicerie · 0 à acheter");
        assert_eq!(casse_de_phrase("À SORTIR : poulet"), "À sortir : poulet");
        assert_eq!(casse_de_phrase("Décision du jour"), "Décision du jour");
        assert_eq!(casse_de_phrase(&ambiance::date_longue("2026-10-24")), "Samedi 24 octobre");
        assert_eq!(casse_de_phrase("L'ENCAISSE EN VIENT"), "L'encaisse en vient");
        assert_eq!(casse_de_phrase(""), "");
        assert_eq!(salut_du_moment(8), "Bonjour");
        assert_eq!(salut_du_moment(14), "Bon après-midi");
        assert_eq!(salut_du_moment(20), "Bonsoir");
        assert_eq!(salut_du_moment(2), "Bonne nuit");
    }

    #[test]
    fn la_grille_prend_autant_de_cartes_qu_il_en_tient() {
        assert_eq!(colonnes_pour(200.0), 1);
        assert_eq!(colonnes_pour(LARGEUR_DE_CARTE * 2.0 + ECART), 2);
        assert_eq!(colonnes_pour(LARGEUR_DE_CARTE * 2.0 + ECART - 1.0), 1);
        assert_eq!(colonnes_pour(2000.0), 3);
    }

    #[test]
    fn un_polygone_se_coupe() {
        let carre = [pos2(0.0, 0.0), pos2(2.0, 0.0), pos2(2.0, 2.0), pos2(0.0, 2.0)];
        let moitie = couper(&carre, |p| 1.0 - p.x);
        assert_eq!(moitie.len(), 4);
        assert!(moitie.iter().all(|p| p.x <= 1.0 + 1e-6));
        assert!(couper(&carre, |p| -1.0 - p.x).is_empty());
        // Un dégradé de CSS à 90° va de la gauche à la droite ; à 135°, du coin en haut à gauche au coin en bas à
        // droite.
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0));
        let deux = [Color32::WHITE, Color32::BLACK];
        assert_eq!(degrade_css(r, 90.0, &deux, r.left_center()), Color32::WHITE);
        assert_eq!(degrade_css(r, 90.0, &deux, r.right_center()), Color32::BLACK);
        assert_eq!(degrade_css(r, 135.0, &deux, r.left_top()), Color32::WHITE);
        assert_eq!(degrade_css(r, 135.0, &deux, r.right_bottom()), Color32::BLACK);
    }
}
