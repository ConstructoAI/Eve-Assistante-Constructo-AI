//! Le thème « Constructo AI » de la fenêtre native : clair, simple et professionnel, habillé comme l'ERP de Constructo
//! AI (`ERP_REACT\frontend`) — l'autre habit de la fenêtre, à côté du centre de commande. Ce qu'il en prend :
//!
//! - le fond qui respire (`src\index.css`, `body` et `.erp-fond-app`, l. 118-131) : un dégradé à −45° du blanc à
//!   `--ui-bg-1` (#BFDBFE) et `--ui-bg-2` (#DBEAFE), quatre fois plus grand que la zone, qui va et vient en 20 s
//!   (`bgBreath`, sur la courbe `ease`) ; figé au milieu de sa course quand Windows ne veut pas d'animations, comme
//!   celui de l'ERP sous `prefers-reduced-motion` (l. 169-177) ;
//! - ses cartes (`.erp-card`, l. 336-340 ; la carte d'indicateur et la carte de section d'`AnalyticsPage.tsx`,
//!   l. 201-265) : blanches, un filet #EDEBE9, une ombre douce, un liseré de couleur à gauche, leur titre en petites
//!   capitales grises, leur icône dans une tuile pastel ; ses badges pastel (`Badge.tsx`) ; son en-tête de page et son
//!   badge du jour (`DashboardWeatherBadge.tsx`) ;
//! - la conversation comme le dock de son assistant (`AssistantDock.tsx`, `MessageBubble.tsx`, `AssistantIAChat.tsx`) :
//!   une colonne à droite que le fond traverse, son en-tête marine (`.erp-topbar`, l. 417-420), les bulles d'Eve
//!   blanches au liseré bleu (`.ai-bubble-assistant`, l. 905-908), celles de la personne en dégradé bleu, la saisie
//!   `.erp-input` (l. 383-392) et le bouton primaire #0078D4 ; le texte des réponses comme son markdown
//!   (`utils\aiMarkdown.ts`) ;
//! - sa palette (`tailwind.config.js`, l. 36-129 ; `index.css`, l. 235-274 et les gris de Fluent, l. 606-670).
//!
//! Pas de réseau neuronal : l'orbe d'Eve est en vedette, sur le fond. Le centre déplié, le tableau de bord à gauche
//! — l'orbe, la décision du jour, les échéances et tout le panneau d'information en cartes ([`crate::centre`]) — et la
//! conversation à droite ; replié (ou la fenêtre étroite), la conversation seule, son orbe en tête. Un thème de bureau,
//! calme comme l'ERP : ni musique ni notes, ni la nuit, les saisons ou l'écran de veille du tableau, ni ses touches.
//! La barre marine reste celle de la fenêtre ([`super::Fenetre`]), comme celle de l'ERP.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Mesh, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, UiBuilder, pos2, vec2,
};
use serde_json::Value;

use super::pieces;
use super::{
    Fenetre, GRAS, ICONES, Ligne, SEMI, Style, TENUE, couleurs, degrade, icones, melange, segments, semi, sphere,
};
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

    /// Une teinte pastel de l'ERP (`Badge.tsx`, les cartes d'indicateurs d'`AnalyticsPage.tsx`) : sa couleur (un
    /// liseré ; à 15 %, le fond d'un badge ou d'une tuile) et son encre.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Teinte {
        pub pastel: Color32,
        pub encre: Color32,
    }

    const fn teinte(pastel: [u8; 3], encre: [u8; 3]) -> Teinte {
        Teinte {
            pastel: Color32::from_rgb(pastel[0], pastel[1], pastel[2]),
            encre: Color32::from_rgb(encre[0], encre[1], encre[2]),
        }
    }

    pub const BLEU: Teinte = teinte([0x7B, 0xAF, 0xD4], [0x4A, 0x7F, 0xA8]);
    pub const VERT: Teinte = teinte([0x7D, 0xC4, 0xA5], [0x4A, 0x94, 0x75]);
    pub const ROUGE: Teinte = teinte([0xE8, 0x91, 0x9A], [0xB8, 0x61, 0x6A]);
    pub const JAUNE: Teinte = teinte([0xF6, 0xC8, 0x7A], [0x9E, 0x7B, 0x1E]);
    pub const AMBRE: Teinte = teinte([0xE8, 0xC1, 0x7A], [0x8B, 0x70, 0x30]);
    pub const ORANGE: Teinte = teinte([0xF0, 0xB0, 0x7A], [0xA0, 0x6A, 0x2A]);
    pub const VIOLET: Teinte = teinte([0xB0, 0x9B, 0xD8], [0x7A, 0x6B, 0xA8]);
    pub const INDIGO: Teinte = teinte([0x8B, 0x9F, 0xD4], [0x5B, 0x6F, 0xA4]);
    pub const SARCELLE: Teinte = teinte([0x7D, 0xC4, 0xB5], [0x4A, 0x94, 0x85]);
    pub const GRIS: Teinte = teinte([0xB8, 0xC4, 0xCE], [0x6B, 0x7B, 0x8A]);
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

/// Une carte de l'ERP (`.erp-card`, la carte d'indicateur du tableau de bord) : blanche, un rayon de 6 points, un filet
/// #EDEBE9, une ombre douce (plus marquée au survol) ; un liseré de 4 points de `accent` à gauche (`border-l-4`). Elle
/// prend toute la largeur donnée et au moins `hauteur` (une rangée d'une grille : ses cartes ont la même hauteur) ;
/// rend la hauteur qu'il lui faut.
pub(crate) fn carte(
    ui: &mut egui::Ui,
    accent: Option<Color32>,
    hauteur: f32,
    contenu: impl FnOnce(&mut egui::Ui),
) -> f32 {
    let fond = ui.painter().add(Shape::Noop);
    let (coin, largeur) = (ui.cursor().min, ui.available_width());
    let marge = Margin { left: if accent.is_some() { 18 } else { 16 }, right: 16, top: 14, bottom: 15 };
    let dedans = Rect::from_min_max(
        coin + vec2(f32::from(marge.left), f32::from(marge.top)),
        pos2(coin.x + largeur - f32::from(marge.right), coin.y + 100_000.0),
    );
    let mut enfant = ui.new_child(UiBuilder::new().max_rect(dedans).layout(Layout::top_down(Align::Min)));
    enfant.spacing_mut().item_spacing.y = 5.0;
    contenu(&mut enfant);
    let naturelle = enfant.min_rect().height() + f32::from(marge.top) + f32::from(marge.bottom);
    let rect = Rect::from_min_size(coin, vec2(largeur, naturelle.max(hauteur)));
    ui.advance_cursor_after_rect(rect);
    let survol = ui.rect_contains_pointer(rect);
    ui.painter().set(fond, fond_de_carte(rect, accent, survol));
    naturelle
}

/// Le fond d'une carte : son ombre (`shadow-sm`, et celle des bulles de l'assistant : 0 2px 8px à 6 % ; au survol,
/// `shadow-lg` : 0 4px 12px à 12 %), sa face blanche et son filet, son liseré.
fn fond_de_carte(rect: Rect, accent: Option<Color32>, survol: bool) -> Shape {
    let rayon: u8 = 6;
    let ombre = |dy: i8, flou: u8, alpha: u8| {
        Shape::Rect(
            egui::Shadow { offset: [0, dy], blur: flou, spread: 0, color: Color32::from_black_alpha(alpha) }
                .as_shape(rect, rayon),
        )
    };
    let mut formes = vec![ombre(1, 2, 13)];
    formes.push(if survol { ombre(4, 12, 30) } else { ombre(2, 8, 15) });
    formes.push(Shape::Rect(egui::epaint::RectShape::new(
        rect,
        rayon,
        Color32::WHITE,
        Stroke::new(1.0, erp::FILET),
        StrokeKind::Inside,
    )));
    if let Some(c) = accent {
        let lisere = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
        let coins = CornerRadius { nw: rayon, sw: rayon, ne: 0, se: 0 };
        formes.push(Shape::Rect(egui::epaint::RectShape::filled(lisere, coins, c)));
    }
    Shape::Vec(formes)
}

/// Une grille de cartes de même hauteur par rangée (celles du tableau de bord de l'ERP, `grid-cols-N gap-4`) : `n`
/// cases sur `colonnes` colonnes, à 16 points l'une de l'autre. `case(ui, k, hauteur)` dessine la k-ième dans sa case,
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
    let (ecart, colonnes) = (16.0, colonnes.max(1));
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

/// Un badge de l'ERP (`Badge.tsx`) : une pilule de la teinte à 18 %, son texte de l'encre de la teinte, en petites
/// capitales demi-grasses.
pub(crate) fn badge(ui: &mut egui::Ui, texte: &str, teinte: erp::Teinte) -> egui::Response {
    let galee = galee_de_badge(ui.painter(), texte, teinte);
    let (rect, rep) = ui.allocate_exact_size(galee.size() + vec2(16.0, 5.0), Sense::hover());
    peindre_badge(ui.painter(), rect, galee, teinte);
    rep
}

/// Le texte d'un badge, mis en page (sa place : sa taille, plus 16 points de large et 5 de haut).
pub(crate) fn galee_de_badge(p: &egui::Painter, texte: &str, teinte: erp::Teinte) -> std::sync::Arc<egui::Galley> {
    p.layout_no_wrap(texte.to_string(), FontId::new(11.0, FontFamily::Name(SEMI.into())), teinte.encre)
}

/// Un badge peint dans `rect`, sa pilule et son texte au milieu.
pub(crate) fn peindre_badge(p: &egui::Painter, rect: Rect, galee: std::sync::Arc<egui::Galley>, teinte: erp::Teinte) {
    p.rect_filled(rect, 255.0, teinte.pastel.gamma_multiply(0.18));
    p.galley(rect.center() - galee.size() / 2.0, galee, teinte.encre);
}

/// La tête d'une carte : son titre en petites capitales grises (`text-xs uppercase tracking-wide`), qui passe à la
/// ligne au besoin, et son icône dans une tuile de sa teinte à droite (`p-2 rounded bg-…/10`) ; en alerte, le titre
/// rouge.
pub(crate) fn tete_de_carte(ui: &mut egui::Ui, titre: &str, icone: char, teinte: erp::Teinte, alerte: bool) {
    let largeur = ui.available_width();
    let tuile = 32.0;
    let encre = if alerte { erp::DANGER_ENCRE } else { erp::TEXTE_2 };
    let mut job = egui::text::LayoutJob::single_section(
        titre.to_uppercase(),
        egui::text::TextFormat {
            font_id: FontId::new(11.5, FontFamily::Name(SEMI.into())),
            color: encre,
            extra_letter_spacing: 0.5,
            ..Default::default()
        },
    );
    job.wrap.max_width = (largeur - tuile - 12.0).max(40.0);
    let galee = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, galee.size().y.max(tuile)), Sense::hover());
    let p = ui.painter();
    p.galley(pos2(rect.left(), rect.top() + 1.0), galee, encre);
    let case = Rect::from_min_size(pos2(rect.right() - tuile, rect.top()), vec2(tuile, tuile));
    p.rect_filled(case, 4.0, teinte.pastel.gamma_multiply(0.16));
    let police = FontId::new(15.0, FontFamily::Name(ICONES.into()));
    p.text(case.center(), Align2::CENTER_CENTER, icone, police, teinte.encre);
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
/// éventail depuis le centre). Pour des dégradés doux, comme ceux des bulles de l'ERP.
fn rectangle_degrade(rect: Rect, rayon: f32, couleur: impl Fn(Pos2) -> Color32) -> Shape {
    let rayon = rayon.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let centres = [
        (pos2(rect.right() - rayon, rect.top() + rayon), -90.0f32),
        (pos2(rect.right() - rayon, rect.bottom() - rayon), 0.0),
        (pos2(rect.left() + rayon, rect.bottom() - rayon), 90.0),
        (pos2(rect.left() + rayon, rect.top() + rayon), 180.0),
    ];
    let mut m = Mesh::default();
    m.colored_vertex(rect.center(), couleur(rect.center()));
    for (c, depart) in centres {
        for k in 0..=4 {
            let a = (depart + k as f32 * 22.5).to_radians();
            let p = c + vec2(a.cos(), a.sin()) * rayon;
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
    let Ligne { de, texte, heure, ancien, pieces, en_cours } = *l;
    let opacite = if ancien { 0.55 } else { 1.0 };
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
                    let couleur = |p: Pos2| degrade_css(r, 90.0, &erp::TOI, p).gamma_multiply(opacite);
                    let ombre =
                        egui::Shadow { offset: [0, 1], blur: 2, spread: 0, color: Color32::from_black_alpha(13) };
                    ui.painter()
                        .set(fond, Shape::Vec(vec![ombre.as_shape(r, 4).into(), rectangle_degrade(r, 4.0, couleur)]));
                    heure_sous(ui, heure, opacite);
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
                    heure_sous(ui, heure, opacite);
                });
            });
            ui.add_space(4.0);
        }
    }
}

/// Le fond d'une bulle d'Eve (`.ai-bubble-assistant`) : son ombre (0 2px 8px à 6 %), son dégradé à 135°, son filet
/// #EDEBE9, son liseré bleu de 4 points ; une alerte, dans le cadre ambre de l'ERP (`.erp-alert-warning`).
fn fond_de_bulle(r: Rect, alerte: bool, opacite: f32) -> Shape {
    let ombre = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(15) };
    let (face, filet, lisere) = if alerte {
        let face = rectangle_degrade(r, 4.0, |_| erp::AVERTISSEMENT_FOND.gamma_multiply(opacite));
        (face, erp::AVERTISSEMENT_BORD, erp::AVERTISSEMENT)
    } else {
        let face = rectangle_degrade(r, 4.0, |p| degrade_css(r, 135.0, &erp::BULLE, p).gamma_multiply(opacite));
        (face, erp::FILET, erp::BULLE_LISERE)
    };
    let contour =
        egui::epaint::RectShape::stroke(r, 4, Stroke::new(1.0, filet.gamma_multiply(opacite)), StrokeKind::Inside);
    let bande = Rect::from_min_size(r.min, vec2(4.0, r.height()));
    let coins = CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 };
    Shape::Vec(vec![
        ombre.as_shape(r, 4).into(),
        face,
        Shape::Rect(contour),
        Shape::Rect(egui::epaint::RectShape::filled(bande, coins, lisere.gamma_multiply(opacite))),
    ])
}

/// La petite orbe d'Eve devant ses bulles (son ombre douce dessous) ; une cloche ambre pour une alerte.
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
    sphere(p, c, 15.0);
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

fn heure_sous(ui: &mut egui::Ui, heure: &str, opacite: f32) {
    if !heure.is_empty() {
        ui.add_space(2.0);
        ui.label(RichText::new(heure).size(11.0).color(erp::TEXTE_3.gamma_multiply(opacite)));
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

/// Ce que le badge du jour dit de la météo, comme celui du tableau de bord de l'ERP (`DashboardWeatherBadge.tsx`,
/// l. 30-51, 163-180) : l'icône et sa teinte selon le temps (le soleil en jaune pâle, la pluie et la neige en bleu,
/// l'orage en orange, le reste en gris), la température, le temps qu'il fait, la ville.
#[derive(Debug, PartialEq)]
struct MeteoDuBadge {
    icone: char,
    teinte: Color32,
    temperature: String,
    temps: String,
    ville: String,
}

/// Le badge du jour d'après la ligne du bulletin (« MONTRÉAL · 5 °C · COUVERT », ou sans la ville), son code WMO et le
/// nom de la ville tel que la famille l'écrit (vide : celui de la ligne).
fn meteo_du_badge(ligne: &str, code: u32, ville: &str) -> Option<MeteoDuBadge> {
    let morceaux: Vec<&str> = ligne.split(" · ").map(str::trim).collect();
    let k = morceaux.iter().position(|m| m.contains('°'))?;
    // « CIEL DÉGAGÉ » → « Ciel dégagé » ; une ville en capitales, « SAINT-JEAN » → « Saint-Jean ».
    let phrase = |m: &str| {
        let bas = m.to_lowercase();
        let mut c = bas.chars();
        c.next().map_or_else(String::new, |p| p.to_uppercase().collect::<String>() + c.as_str())
    };
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
        0 | 1 => (pictos::SOLEIL, erp::JAUNE.pastel),
        2 => (pictos::SOLEIL_VOILE, erp::JAUNE.pastel),
        51..=67 | 71..=77 | 80..=86 => (pictos::NUAGE, erp::BLEU.pastel),
        95..=99 => (pictos::NUAGE, erp::ORANGE.pastel),
        _ => (pictos::NUAGE, erp::TEXTE_3),
    };
    Some(MeteoDuBadge {
        icone,
        teinte,
        temperature: morceaux[k].to_string(),
        temps: morceaux.get(k + 1).map(|m| phrase(m)).unwrap_or_default(),
        ville,
    })
}

/// « SAMEDI 24 OCTOBRE » → « samedi 24 octobre » : la date longue du Canada français, comme l'ERP l'écrit.
fn date_du_badge(aujourdhui: &str) -> String {
    ambiance::date_longue(aujourdhui).to_lowercase()
}

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

    /// La fenêtre sous la barre, dans le thème « Constructo AI » : le fond qui respire ; le centre déplié, le tableau
    /// de bord, et la conversation à droite comme le dock de l'assistant de l'ERP ; replié (ou la fenêtre étroite), la
    /// conversation seule, son orbe en tête.
    pub(super) fn constructo(&mut self, ui: &mut egui::Ui) {
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
            ui.painter().rect_filled(tuile, 4.0, Color32::from_white_alpha(38));
            sphere(ui.painter(), tuile.center(), 10.0);
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

    /// Le haut de la conversation seule : l'orbe, ce qu'Eve fait (comme l'Eve de Constructo AI dans son thème clair :
    /// 19 points en gras), comment lui parler ; la demande d'accord, s'il y en a une.
    fn tete_claire(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            self.orbe(ui);
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

    /// Le tableau de bord, comme celui de l'ERP (`AnalyticsPage.tsx`) : son titre et le badge du jour ; l'orbe en
    /// vedette et la décision du jour à gauche ; à droite, les échéances et tout le panneau d'information en cartes.
    /// Dans une place étroite, tout en une colonne. Chaque colonne défile si elle déborde.
    fn tableau_de_bord(&mut self, ui: &mut egui::Ui) {
        // Les marges de l'ERP (`p-6`) ; en bas, les colonnes défilent jusqu'au bord de la fenêtre, comme une page.
        let m = ui.max_rect();
        let r = Rect::from_min_max(m.min + vec2(24.0, 22.0), pos2(m.right() - 24.0, m.bottom()));
        let aujourdhui = journal::date();
        let horodatage = journal::horodatage();
        let heure = horodatage.get(11..16).unwrap_or("").replace('-', ":");
        ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
            ui.horizontal(|ui| {
                let titre = RichText::new("Tableau de bord").family(FontFamily::Name(GRAS.into())).size(24.0);
                ui.label(titre.color(erp::TEXTE));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.badge_du_jour(ui, &aujourdhui, &horodatage);
                });
            });
            ui.add_space(14.0);
            let corps = ui.available_rect_before_wrap();
            let colonne = |ui: &mut egui::Ui, place: Rect, nom: &str, contenu: &mut dyn FnMut(&mut egui::Ui)| {
                ui.scope_builder(UiBuilder::new().max_rect(place), |ui| {
                    let sortie = egui::ScrollArea::vertical().id_salt(nom).auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(4.0);
                        contenu(ui);
                        ui.add_space(24.0);
                    });
                    // Défilée, la colonne s'estompe sous le titre au lieu d'y être coupée net.
                    if sortie.state.clamped_offset().y > 1.0 {
                        fondu(ui.painter(), Rect::from_min_size(place.min, vec2(place.width(), 18.0)));
                    }
                });
            };
            if corps.width() >= 860.0 {
                let l = (corps.width() * 0.36).clamp(320.0, 460.0);
                let gauche = Rect::from_min_size(corps.min, vec2(l, corps.height()));
                let droite = Rect::from_min_max(pos2(gauche.right() + 24.0, corps.top()), corps.max);
                let hauteur = gauche.height() - 28.0;
                colonne(ui, gauche, "colonne-du-jour", &mut |ui| self.colonne_du_jour(ui, &aujourdhui, hauteur));
                colonne(ui, droite, "colonne-des-cartes", &mut |ui| self.cartes_du_tableau(ui, &aujourdhui, &heure));
            } else {
                colonne(ui, corps, "colonne-unique", &mut |ui| {
                    self.colonne_du_jour(ui, &aujourdhui, 0.0);
                    ui.add_space(16.0);
                    self.cartes_du_tableau(ui, &aujourdhui, &heure);
                });
            }
        });
    }

    /// La colonne du jour : l'orbe en vedette sur le fond, ce qu'Eve fait et comment lui parler, puis la décision du
    /// jour. Centrée dans `hauteur` (la colonne), d'après sa hauteur à l'image d'avant ; 0 : en haut.
    fn colonne_du_jour(&mut self, ui: &mut egui::Ui, aujourdhui: &str, hauteur: f32) {
        let id = egui::Id::unique("hauteur-de-la-colonne-du-jour");
        let avant = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
        ui.add_space(((hauteur - avant) / 2.0 - 12.0).max(0.0));
        let debut = ui.cursor().top();
        ui.vertical_centered(|ui| {
            self.orbe(ui);
            let etat = RichText::new(self.champ("libelle")).family(FontFamily::Name(GRAS.into())).size(19.0);
            ui.label(etat.color(erp::TEXTE));
            let aide = self.champ("aide");
            if !aide.is_empty() {
                ui.label(RichText::new(aide).size(13.0).color(erp::TEXTE_2));
            }
        });
        ui.add_space(22.0);
        self.centre.decision_carte(ui, aujourdhui);
        let mesure = ui.cursor().top() - debut;
        if (mesure - avant).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, mesure));
            ui.ctx().request_repaint();
        }
    }

    /// Les cartes du tableau de bord : les échéances, puis le panneau d'information, en deux ou trois colonnes selon la
    /// place.
    fn cartes_du_tableau(&mut self, ui: &mut egui::Ui, aujourdhui: &str, heure: &str) {
        self.centre.echeances_carte(ui, aujourdhui);
        let colonnes = ((ui.available_width() + 16.0) / (290.0 + 16.0)).floor().clamp(1.0, 3.0) as usize;
        self.centre.panneau_cartes(ui, aujourdhui, heure, colonnes);
    }

    /// Le badge du jour, comme celui du tableau de bord de l'ERP : la date et l'heure, puis, quand la météo est là, son
    /// icône, la température, le temps qu'il fait et la ville.
    fn badge_du_jour(&self, ui: &mut egui::Ui, aujourdhui: &str, horodatage: &str) {
        let ville = self.meteo.as_ref().map(|m| m.lieu().ville.clone()).unwrap_or_default();
        let meteo = self.bulletin.as_ref().and_then(|b| meteo_du_badge(&b.ligne, b.code, &ville));
        // Ses morceaux, de gauche à droite, mis en page d'abord : le badge épouse leur largeur.
        let (normale, demi) = (FontId::proportional(13.5), FontId::new(13.5, FontFamily::Name(SEMI.into())));
        let p = ui.painter();
        let jour = format!("{} · {}", date_du_badge(aujourdhui), ambiance::heure_de_veille(horodatage));
        let mut morceaux = vec![p.layout_no_wrap(jour, normale.clone(), erp::TEXTE_2)];
        if let Some(m) = meteo {
            morceaux.push(p.layout_no_wrap("|".into(), normale.clone(), Color32::from_rgb(0xC8, 0xC6, 0xC4)));
            let icone = FontId::new(15.0, FontFamily::Name(ICONES.into()));
            morceaux.push(p.layout_no_wrap(m.icone.to_string(), icone, m.teinte));
            morceaux.push(p.layout_no_wrap(m.temperature, demi, erp::TEXTE));
            for mot in [m.temps, m.ville].into_iter().filter(|x| !x.is_empty()) {
                morceaux.push(p.layout_no_wrap(mot, normale.clone(), erp::TEXTE_2));
            }
        }
        let ecart = 7.0;
        let largeur = morceaux.iter().map(|g| g.size().x).sum::<f32>() + ecart * (morceaux.len() - 1) as f32 + 22.0;
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, 32.0), Sense::hover());
        let p = ui.painter();
        p.rect(rect, 4.0, Color32::WHITE, Stroke::new(1.0, erp::FILET), StrokeKind::Inside);
        let mut x = rect.left() + 11.0;
        for g in morceaux {
            let l = g.size().x;
            p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, erp::TEXTE);
            x += l + ecart;
        }
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
    fn le_badge_du_jour_comme_l_erp() {
        let m = meteo_du_badge("LAVAL · 3 °C · CIEL DÉGAGÉ", 0, "").unwrap();
        assert_eq!(
            m,
            MeteoDuBadge {
                icone: pictos::SOLEIL,
                teinte: erp::JAUNE.pastel,
                temperature: "3 °C".into(),
                temps: "Ciel dégagé".into(),
                ville: "Laval".into()
            }
        );
        let m = meteo_du_badge("SAINT-JEAN · -2 °C · NEIGE MODÉRÉE", 73, "Saint-Jean-sur-Richelieu").unwrap();
        assert_eq!(
            (m.ville.as_str(), m.temps.as_str(), m.teinte),
            ("Saint-Jean-sur-Richelieu", "Neige modérée", erp::BLEU.pastel)
        );
        let m = meteo_du_badge("5 °C · COUVERT", 3, "").unwrap();
        assert_eq!((m.ville.as_str(), m.temperature.as_str(), m.teinte), ("", "5 °C", erp::TEXTE_3));
        assert_eq!(meteo_du_badge("", 0, ""), None);
        assert_eq!(date_du_badge("2026-10-24"), "samedi 24 octobre");
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
