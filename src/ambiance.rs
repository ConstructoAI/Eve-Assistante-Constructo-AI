//! L'ambiance du centre de commande (le lot 6) : ce que le tableau HTML de l'Assistante famille
//! (`centre-de-commande.html`, aux archives, cité « cdc:<ligne> ») pose sur son réseau sans qu'aucun outil
//! ne le demande, et son écran de veille. `natif.rs` en dessine deux couches, comme la page en avait deux :
//!
//! 1. [`Ambiance::dessiner`], après le réseau et l'orbe, sous les panneaux — dans la page, la fin de la toile, puis les
//!    calques `#scan` et `#nuit`, sous les textes : la saison, la fête, le balayage, la nuit ;
//! 2. [`Ambiance::veille`], par-dessus le reste : l'écran de veille, quand il est dû.
//!
//! Ce que la page en fait, et qu'on refait au nombre près :
//! - **la saison** (cdc:745-767) : à l'Halloween, du 20 octobre au 1er novembre, une lueur orange qui respire sur le
//!   bas et 70 braises qui montent en vacillant ; de décembre à mars, ou quand la météo dit qu'il neige, la neige ; à
//!   Noël, du 18 au 26 décembre, la neige et des étoiles filantes ;
//! - **la fête** (cdc:779-789) : le jour d'un anniversaire, 90 confettis qui tombent en tournant, la moitié de la
//!   couleur de la personne, l'autre d'un blanc chaud ; et le souhait que la page tapait sur sa ligne d'état
//!   ([`Ambiance::souhait`]) ;
//! - **le balayage** (cdc:41) : une fine ligne cyan tous les trois points, sur tout le centre ;
//! - **la nuit** (cdc:61-62, 741-743) : de 21 h à 6 h, un voile bleu nuit qui vient et s'en va en 4 s (la musique,
//!   elle, baisse d'elle-même : `musique.rs`) ;
//! - **l'écran de veille** (cdc:50-53, 574-583) : après 90 s sans un geste ni un outil, la grande horloge, la date, la
//!   météo et la prochaine échéance viennent au centre en 2,5 s, dans leur lueur cyan ; la souris les chasse. La page
//!   ne cache rien pendant la veille : le réseau, les panneaux et le flux restent dessous.
//!
//! En mode économe, ni saison ni confettis (cdc:430-432, 784), comme la page. Elle avançait ses particules d'un pas par
//! image, à 30 images par seconde au plus (cdc:339-341) ; la fenêtre choisit sa cadence : elles avancent ici du temps
//! écoulé, à la vitesse qu'elles avaient dans la page. Ses fondus sont ceux du navigateur : la courbe `ease` des
//! transitions CSS, et un fondu interrompu qui repart d'où il en est. La lueur de ses textes (`text-shadow`, un flou de
//! 18 px) : sous l'horloge, des copies du texte décalées selon une gaussienne ; sous les petites lignes, dont ce flou
//! fond les lettres ensemble, le flou de chaque mot, calculé.
//!
//! Ce qui est propre à une famille — ses personnes, leurs anniversaires, leurs couleurs — vient de l'objet
//! `EVE_FAMILLE` que la fenêtre a lu ([`fete_du_jour`]) : rien n'en est écrit ici.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::epaint::TextShape;
use eframe::egui::epaint::text::Glyph;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Align, Color32, Event, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use serde_json::Value;

use crate::natif::hud;

/// Les braises de l'Halloween et les flocons de l'hiver (les mêmes particules, cdc:746), les confettis (cdc:782).
const PARTICULES: usize = 70;
const CONFETTIS: usize = 90;
/// Les pas de la page, d'une image à l'autre (30 par seconde, cdc:340), devenus des vitesses : en hauteurs du centre
/// par seconde et par unité de vitesse d'une particule. Les braises montent de 0,0012 par image (cdc:759), la neige
/// tombe de 0,0016 (cdc:762), les confettis de 0,0018 (cdc:785).
const MONTEE_DES_BRAISES: f32 = 0.0012 * 30.0;
const CHUTE_DE_LA_NEIGE: f32 = 0.0016 * 30.0;
const CHUTE_DES_CONFETTIS: f32 = 0.0018 * 30.0;
/// L'étoile filante de Noël (cdc:764-765) : quand le ciel n'en a pas, 0,6 % de chances par image qu'il en naisse une
/// (0,18 par seconde) ; elle file de 8 points vers la gauche et de 4 vers le bas par image, sa traîne de 90 points sur
/// 45 derrière elle, et s'éteint en 40 images.
const NAISSANCES_D_ETOILES: f64 = 0.006 * 30.0;
const VITESSE_DE_L_ETOILE: Vec2 = vec2(-8.0 * 30.0, 4.0 * 30.0);
const TRAINE_DE_L_ETOILE: Vec2 = vec2(90.0, -45.0);
const VIE_DE_L_ETOILE: f64 = 40.0 / 30.0;
/// Après une absence (la fenêtre réduite, le centre replié), les particules ne sautent pas : elles avancent d'au plus
/// ce temps d'une image à l'autre.
const PAS_MAX: f64 = 0.25;
/// Le voile de nuit (cdc:61) et son fondu.
const VOILE_DE_NUIT: f32 = 0.4;
const FONDU_DE_NUIT: f64 = 4.0;
/// La veille vient après 90 s sans activité (cdc:575), en 2,5 s (cdc:50), à 120 points du haut (cdc:50).
const ATTENTE_DE_VEILLE: f64 = 90.0;
const FONDU_DE_VEILLE: f64 = 2.5;
const HAUT_DE_VEILLE: f32 = 120.0;
/// Une veille qui ne s'était plus dessinée depuis ce temps (le centre replié, la fenêtre réduite) : y revenir est une
/// activité.
const ABSENCE: f64 = 2.0;
/// Le souhait d'anniversaire se tape 2,5 s après l'ouverture (cdc:789).
const SOUHAIT_APRES: f64 = 2.5;
/// La lueur des textes de la veille, `text-shadow: 0 0 18px` (cdc:50) : un flou gaussien. La norme CSS lui donne pour
/// écart type la moitié du rayon ; Chromium, qui dessine la page dans Edge, 0,288675 fois le rayon, plus 0,5 (la
/// conversion de Skia, d'un demi-rayon) : c'est ce flou-là que la page montre, 5,7 points.
const SIGMA: f32 = 0.288_675 * 18.0 + 0.5;
/// L'horloge, au-delà de [`GRAND`] points, garde la forme de ses chiffres dans sa lueur : elle la prend de [`COPIES`]
/// copies d'elle-même, chacune d'un 96e de la lueur (une marche de 1 % au bord de chacune, que l'œil ne voit pas).
const GRAND: f32 = 40.0;
const COPIES: usize = 96;
/// Une petite ligne, ce flou fond ses lettres (à leur pas, il n'en laisse que 2 %) : elle prend le flou de ses mots,
/// chacun la bande de ses capitales, à la part d'encre que leur laisse leur espacement. Une capitale de Consolas encre
/// 0,12 em² en moyenne (mesuré sur les 26 lettres, et autant pour les chiffres), dans sa chasse de 0,55 em, haute de
/// 0,638 em.
const ENCRE_D_UNE_CAPITALE: f32 = 0.12;
const CHASSE: f32 = 0.55;
const CAPITALES: f32 = 0.638;
/// La graine des particules : la page tirait `Math.random()`, une autre place à chaque ouverture ; une graine fixe rend
/// les captures des essais pareilles d'une fois à l'autre.
const GRAINE: u64 = 20_261_024;

type Rgb = [u8; 3];

/// Les couleurs de la page : la lueur de l'Halloween (cdc:758), la neige (cdc:762), l'étoile filante (cdc:765), le
/// voile de nuit (cdc:61), le cyan du balayage et des lueurs (cdc:7, 41) ; la grande horloge (cdc:52) et la date
/// (cdc:52, le bleu de l'horloge du tableau, cdc:18).
const ORANGE: Rgb = [255, 120, 20];
const NEIGE: Rgb = [225, 240, 255];
const ETOILE: Rgb = [255, 236, 170];
const NUIT: Rgb = [0, 0, 12];
const CYAN: Rgb = [0, 229, 255];
const BLANC: Color32 = Color32::from_rgb(0xE8, 0xFB, 0xFF);
const BLEU: Color32 = Color32::from_rgb(0x7F, 0xB4, 0xD4);
/// La couleur d'une personne qui n'a pas la sienne : celle de son quartier du réseau, par son rang, dans l'ordre de la
/// page (cdc:531).
const QUARTIERS: [Rgb; 4] = [[0, 229, 255], [255, 122, 184], [255, 210, 63], [95, 255, 196]];
const JOURS: [&str; 7] = ["DIMANCHE", "LUNDI", "MARDI", "MERCREDI", "JEUDI", "VENDREDI", "SAMEDI"];
const MOIS: [&str; 12] = [
    "JANVIER", "FÉVRIER", "MARS", "AVRIL", "MAI", "JUIN", "JUILLET", "AOÛT", "SEPTEMBRE", "OCTOBRE", "NOVEMBRE",
    "DÉCEMBRE",
];

// ---- Ce que la fenêtre donne, et ce qui se calcule sans elle -------------------------------------------------------

/// Le moment d'une image : ce qui décide de la saison, de la fête et de la nuit.
pub struct Moment<'a> {
    /// Le jour, « 2026-10-24 » (`journal::date`).
    pub date: &'a str,
    /// L'heure, de 0 à 23 : la nuit va de 21 h à 6 h (cdc:742).
    pub heure: u32,
    /// Le mode économe du réseau : ni saison ni confettis (cdc:431, 784).
    pub econome: bool,
    /// La météo dit qu'il neige (la page : les codes WMO 71, 73, 75, 77, 85 et 86, cdc:752).
    pub il_neige: bool,
    /// L'anniversaire du jour, s'il y en a un ([`fete_du_jour`]).
    pub fete: Option<&'a Fete>,
}

/// Quelqu'un dont c'est l'anniversaire : son nom, tel que la famille l'écrit, et sa couleur.
#[derive(Clone, Debug, PartialEq)]
pub struct Fete {
    pub nom: String,
    pub couleur: [u8; 3],
}

/// Ce que dit l'écran de veille, ligne à ligne ; une ligne vide ne prend pas de place.
pub struct Veille<'a> {
    /// L'heure, « 09 h 05 » ([`heure_de_veille`]).
    pub heure: &'a str,
    /// La date, « SAMEDI 24 OCTOBRE » ([`date_longue`]).
    pub date: &'a str,
    /// La météo, telle que le tableau l'écrit en haut à droite : « VILLE · 5 °C · COUVERT ».
    pub meteo: &'a str,
    /// La prochaine échéance ([`prochaine_echeance`]).
    pub echeance: &'a str,
}

/// La saison qu'un jour fait voir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saison {
    /// La lueur orange et les braises.
    Halloween,
    /// La neige et les étoiles filantes.
    Noel,
    Neige,
}

/// La saison d'un jour (cdc:748-754) : l'Halloween du 20 octobre au 1er novembre, Noël du 18 au 26 décembre, la neige
/// de décembre à mars ou quand il neige (l'Halloween et Noël passent devant) ; rien le reste du temps.
pub fn saison(date: &str, il_neige: bool) -> Option<Saison> {
    let neige = il_neige.then_some(Saison::Neige);
    let Some((_, mois, jour)) = jour_de(date) else { return neige };
    match (mois, jour) {
        (10, 20..=31) | (11, 1) => Some(Saison::Halloween),
        (12, 18..=26) => Some(Saison::Noel),
        (12 | 1..=3, _) => Some(Saison::Neige),
        _ => neige,
    }
}

/// La nuit du tableau, de 21 h à 6 h (cdc:742).
fn est_la_nuit(heure: u32) -> bool {
    !(6..21).contains(&heure)
}

/// L'anniversaire du jour, d'après `EVE_FAMILLE.personnes` : la première personne dont l'anniversaire (« MM-JJ », ou
/// « AAAA-MM-JJ ») tombe ce jour-là, comme la page prenait la première de sa liste (cdc:781) ; né un 29 février, on
/// fête le 28 les années communes. Sa couleur : la sienne (`[r, v, b]`, ou « #rrvvbb »), sinon celle de son quartier.
/// Une personne sans nom n'a pas de fête : il n'y aurait personne à qui la souhaiter.
pub fn fete_du_jour(famille: &Value, date: &str) -> Option<Fete> {
    let (annee, mois, jour) = jour_de(date)?;
    let bissextile = annee % 4 == 0 && (annee % 100 != 0 || annee % 400 == 0);
    famille.get("personnes")?.as_array()?.iter().enumerate().find_map(|(rang, p)| {
        let nom = p.get("nom")?.as_str()?.trim();
        let (m, j) = anniversaire(p.get("anniversaire")?.as_str()?)?;
        let (m, j) = if (m, j) == (2, 29) && !bissextile { (2, 28) } else { (m, j) };
        (!nom.is_empty() && (m, j) == (mois, jour)).then(|| Fete {
            nom: nom.to_string(),
            couleur: p.get("couleur").and_then(rgb_de).unwrap_or(QUARTIERS[rang % QUARTIERS.len()]),
        })
    })
}

/// « 2026-10-24 » → « SAMEDI 24 OCTOBRE » : le jour, son numéro et le mois, en capitales (cdc:578, la date longue du
/// Canada français, sans « 1er »). Une date mal formée reste telle quelle.
pub fn date_longue(date: &str) -> String {
    match jour_de(date) {
        Some((a, m, j)) => format!("{} {j} {}", JOURS[jour_de_la_semaine(a, m, j)], MOIS[m as usize - 1]),
        None => date.to_string(),
    }
}

/// L'heure de la veille, « 09 h 05 » (cdc:577 : l'heure et les minutes sur deux chiffres, à la manière du Canada
/// français), d'après l'horodatage du journal (« 2026-10-24 09-05-00 ») ; rien s'il est mal formé.
pub fn heure_de_veille(horodatage: &str) -> String {
    match (horodatage.get(11..13), horodatage.get(14..16)) {
        (Some(h), Some(m)) if h.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit()) => format!("{h} h {m}"),
        _ => String::new(),
    }
}

/// La ligne de la prochaine échéance (cdc:580) : « PROCHAINE ÉCHÉANCE · DANS 7 J · PNEUS D'HIVER ». `jours` :
/// d'aujourd'hui à son jour (la page prend la plus proche, de 0 à 150 jours, cdc:554-557).
pub fn prochaine_echeance(jours: i64, texte: &str) -> String {
    let quand = match jours {
        0 => "AUJOURD'HUI".to_string(),
        1 => "DEMAIN".to_string(),
        n => format!("DANS {n} J"),
    };
    format!("PROCHAINE ÉCHÉANCE · {quand} · {}", texte.trim().to_uppercase())
}

/// Le souhait de la page (cdc:789), « JOYEUX ANNIVERSAIRE, ZOÉ! ».
fn souhait_a(nom: &str) -> String {
    format!("JOYEUX ANNIVERSAIRE, {}!", nom.trim().to_uppercase())
}

// ---- L'ambiance ---------------------------------------------------------------------------------------------------

/// Une braise ou un flocon (cdc:746) : sa place (en parts du centre), sa vitesse, sa taille, sa phase.
struct Particule {
    x: f32,
    y: f32,
    v: f32,
    r: f32,
    ph: f32,
}

/// Un confetti (cdc:782) : sa place, sa vitesse, sa largeur, sa phase, et de quoi choisir sa couleur (sous 0,5, celle
/// de la personne).
struct Confetti {
    x: f32,
    y: f32,
    v: f32,
    w: f32,
    ph: f32,
    c: f32,
}

/// Une étoile filante : où elle est née (en parts du centre), et quand.
struct Etoile {
    x: f32,
    y: f32,
    nee: f64,
}

/// La fête vue : à qui, depuis quand, et si son souhait est déjà rendu.
struct FeteVue {
    nom: String,
    depuis: f64,
    souhaitee: bool,
}

/// La lueur d'une petite ligne de la veille, tirée à pleine force pour un texte à une place : elle ne se recalcule que
/// quand l'un ou l'autre change (l'horloge, chaque minute).
struct LueurDeLigne {
    texte: String,
    place: Pos2,
    maillage: Arc<Mesh>,
}

/// L'état de l'ambiance d'une image à l'autre : ses particules, ses fondus, l'activité que la veille attend.
pub struct Ambiance {
    hasard: Hasard,
    /// Les braises de l'Halloween et les flocons de l'hiver : les mêmes particules, comme dans la page.
    particules: Vec<Particule>,
    confettis: Vec<Confetti>,
    etoile: Option<Etoile>,
    /// L'heure de la dernière image : les particules avancent du temps écoulé depuis.
    avant: Option<f64>,
    /// Le voile de nuit, posé sans fondu à la première image (la page s'ouvre déjà dans sa nuit).
    nuit: Option<Fondu>,
    /// Les lignes du balayage, et le rectangle pour lequel elles sont tirées : le même maillage d'une image à l'autre.
    balayage: Option<(Rect, Arc<Mesh>)>,
    fete: Option<FeteVue>,
    /// La dernière activité ; la dernière image où la veille s'est demandée ; la dernière place de la souris ; son
    /// fondu.
    activite: Option<f64>,
    vue: Option<f64>,
    souris: Option<Pos2>,
    veille: Fondu,
    /// Les décalages des copies qui font la lueur de l'horloge ; la lueur de chaque ligne de la veille, une fois tirée.
    copies: Vec<Vec2>,
    lueurs: [Option<LueurDeLigne>; 4],
}

impl Ambiance {
    pub fn nouvelle() -> Ambiance {
        let mut hasard = Hasard(GRAINE);
        let particules = (0..PARTICULES)
            .map(|_| Particule {
                x: hasard.f(),
                y: hasard.f(),
                v: hasard.entre(0.3, 1.1),
                r: hasard.entre(0.7, 2.3),
                ph: hasard.entre(0.0, 6.28),
            })
            .collect();
        let confettis = (0..CONFETTIS)
            .map(|_| Confetti {
                x: hasard.f(),
                y: hasard.f(),
                v: hasard.entre(0.4, 1.3),
                w: hasard.entre(3.0, 7.0),
                ph: hasard.entre(0.0, 6.28),
                c: hasard.f(),
            })
            .collect();
        Ambiance {
            hasard,
            particules,
            confettis,
            etoile: None,
            avant: None,
            nuit: None,
            balayage: None,
            fete: None,
            activite: None,
            vue: None,
            souris: None,
            veille: Fondu::fixe(0.0),
            copies: copies_de_la_lueur(),
            lueurs: Default::default(),
        }
    }

    /// Dans `rect` (le centre de commande, sous ses panneaux), après le réseau et l'orbe : la saison, la fête, le
    /// balayage et la nuit du moment ; `temps` : l'horloge d'egui (`ui.input(|i| i.time)`). Rend vrai tant que quelque
    /// chose bouge — des particules, un fondu de la nuit ou de la veille : la fenêtre redessine alors à sa cadence.
    pub fn dessiner(&mut self, p: &Painter, rect: Rect, temps: f64, moment: &Moment) -> bool {
        let dt = self.avant.map_or(0.0, |a| (temps - a).clamp(0.0, PAS_MAX)) as f32;
        self.avant = Some(temps);
        self.suivre_la_fete(moment.fete, temps);
        let noire = if est_la_nuit(moment.heure) { 1.0 } else { 0.0 };
        let nuit = self.nuit.get_or_insert(Fondu::fixe(noire));
        nuit.viser(noire, temps, FONDU_DE_NUIT);
        let voile = nuit.valeur(temps);
        let mut bouge = nuit.bouge(temps) || self.veille.bouge(temps);
        if !rect.is_positive() || !rect.is_finite() {
            return bouge;
        }
        let p = p.with_clip_rect(rect);
        // La saison et la fête : ni l'une ni l'autre en mode économe (cdc:431-432, 784).
        let la_saison = if moment.econome { None } else { saison(moment.date, moment.il_neige) };
        match la_saison {
            Some(Saison::Halloween) => self.peindre_l_halloween(&p, rect, temps, dt),
            Some(s) => {
                self.peindre_la_neige(&p, rect, temps, dt);
                if s == Saison::Noel {
                    self.peindre_l_etoile(&p, rect, temps, dt);
                }
            }
            None => {}
        }
        if la_saison != Some(Saison::Noel) {
            self.etoile = None;
        }
        if let Some(f) = moment.fete.filter(|_| !moment.econome) {
            self.peindre_les_confettis(&p, rect, temps, dt, f.couleur);
            bouge = true;
        }
        bouge |= la_saison.is_some();
        self.peindre_le_balayage(&p, rect);
        if voile > 0.0 {
            p.rect_filled(rect, 0.0, voile_de(NUIT, VOILE_DE_NUIT * voile));
        }
        bouge
    }

    /// Une activité de la personne ou d'Eve, à l'instant `temps` : un outil qui part ou revient, une phrase entendue,
    /// Eve qui parle. La veille attend 90 s sans rien. La souris et le clavier de la fenêtre, [`Ambiance::veille`] les
    /// voit elle-même.
    pub fn signal(&mut self, temps: f64) {
        self.activite = Some(temps);
    }

    /// L'écran de veille, par-dessus le reste, dans `rect` (le centre de commande) : il vient après 90 s sans activité,
    /// quand rien ne le retient — `bloquee` : un outil en cours, la démonstration ou la journée rejouée (cdc:575), Eve
    /// qui écoute ou qui parle —, et la souris le chasse (cdc:583), comme un bouton, la molette ou une touche. Rend son
    /// opacité : 0, rien n'est dessiné.
    pub fn veille(&mut self, p: &Painter, rect: Rect, temps: f64, bloquee: bool, contenu: &Veille) -> f32 {
        let mut geste = false;
        let souris = &mut self.souris;
        p.ctx().input(|i| {
            for e in &i.events {
                geste |= match e {
                    // Une souris qui ne bouge pas (Windows en redit parfois la place) n'est pas un geste.
                    Event::PointerMoved(ici) => souris.replace(*ici).is_some_and(|la| la.distance(*ici) > 0.5),
                    Event::PointerButton { .. } | Event::MouseWheel { .. } | Event::Text(_) => true,
                    Event::Key { pressed, .. } => *pressed,
                    _ => false,
                };
            }
        });
        let revenue = self.vue.is_some_and(|v| temps - v > ABSENCE);
        self.vue = Some(temps);
        if geste || revenue || self.activite.is_none() {
            self.signal(temps);
        }
        let due = !bloquee && self.activite.is_some_and(|a| temps - a >= ATTENTE_DE_VEILLE);
        self.veille.viser(if due { 1.0 } else { 0.0 }, temps, FONDU_DE_VEILLE);
        // Le fondu demande lui-même ses images (30 par seconde) : il commence souvent après `dessiner`, qui ne pouvait
        // pas encore le dire.
        if self.veille.bouge(temps) {
            p.ctx().request_repaint_after(Duration::from_millis(33));
        }
        let opacite = self.veille.valeur(temps);
        if opacite > 0.0 && rect.is_positive() && rect.is_finite() {
            self.peindre_la_veille(p, rect, opacite, contenu);
        }
        opacite
    }

    /// Le souhait d'anniversaire à taper sur la ligne d'état, « JOYEUX ANNIVERSAIRE, ZOÉ! » : une seule fois, 2,5 s
    /// après que la fête est venue (l'ouverture du centre, ou minuit) ; sinon rien.
    pub fn souhait(&mut self, temps: f64) -> Option<String> {
        let f = self.fete.as_mut().filter(|f| !f.souhaitee && temps >= f.depuis + SOUHAIT_APRES)?;
        f.souhaitee = true;
        Some(souhait_a(&f.nom))
    }

    fn suivre_la_fete(&mut self, fete: Option<&Fete>, temps: f64) {
        match fete {
            Some(f) if self.fete.as_ref().is_some_and(|vue| vue.nom == f.nom) => {}
            Some(f) => self.fete = Some(FeteVue { nom: f.nom.clone(), depuis: temps, souhaitee: false }),
            None => self.fete = None,
        }
    }

    /// L'Halloween (cdc:757-760) : une lueur orange sur le bas, de 10 % (qui respire de ±4 %) au bas à rien aux 55 % de
    /// la hauteur ; et les braises, de petits carrés qui montent en vacillant, du rouge à l'orange.
    fn peindre_l_halloween(&mut self, p: &Painter, rect: Rect, temps: f64, dt: f32) {
        let lueur = voile_de(ORANGE, 0.10 + 0.04 * (temps * 1.7).sin() as f32);
        let haut = rect.top() + rect.height() * 0.55;
        let mut m = Mesh::default();
        m.colored_vertex(pos2(rect.left(), haut), Color32::TRANSPARENT);
        m.colored_vertex(pos2(rect.right(), haut), Color32::TRANSPARENT);
        m.colored_vertex(rect.right_bottom(), lueur);
        m.colored_vertex(rect.left_bottom(), lueur);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(Shape::mesh(m));
        let (l, h) = (rect.width(), rect.height());
        for q in &mut self.particules {
            q.y -= q.v * MONTEE_DES_BRAISES * dt;
            if q.y < -0.02 {
                q.y = 1.02;
                q.x = self.hasard.f();
            }
            let ph = f64::from(q.ph);
            // Une opacité qui passe sous zéro, CSS la ramène à zéro : la braise s'éteint un moment.
            let force = (0.25 + 0.3 * (temps * 2.0 + ph).sin()) as f32;
            if force <= 0.0 {
                continue;
            }
            let vert = (110.0 + 80.0 * (temps * 3.0 + ph).sin()) as u8;
            let coin = pos2(rect.left() + q.x * l + (12.0 * (temps + ph).sin()) as f32, rect.top() + q.y * h);
            let cote = q.r * 1.3;
            p.rect_filled(Rect::from_min_size(coin, vec2(cote, cote)), 0.0, voile_de([255, vert, 30], force));
        }
    }

    /// La neige (cdc:762) : des flocons qui tombent en se balançant, plus opaques quand ils sont plus gros.
    fn peindre_la_neige(&mut self, p: &Painter, rect: Rect, temps: f64, dt: f32) {
        let (l, h) = (rect.width(), rect.height());
        for q in &mut self.particules {
            q.y += q.v * CHUTE_DE_LA_NEIGE * dt;
            if q.y > 1.02 {
                q.y = -0.02;
                q.x = self.hasard.f();
            }
            let x = rect.left() + q.x * l + (18.0 * (temps * 0.6 + f64::from(q.ph)).sin()) as f32;
            p.circle_filled(pos2(x, rect.top() + q.y * h), q.r, voile_de(NEIGE, 0.25 + 0.35 * q.r / 2.3));
        }
    }

    /// L'étoile filante de Noël (cdc:764-765) : née au hasard dans le haut du ciel, plutôt à droite, elle file vers le
    /// bas à gauche en s'éteignant.
    fn peindre_l_etoile(&mut self, p: &Painter, rect: Rect, temps: f64, dt: f32) {
        if self.etoile.is_none() && self.hasard.suivant() < 1.0 - (-NAISSANCES_D_ETOILES * f64::from(dt)).exp() {
            self.etoile = Some(Etoile { x: 0.35 + 0.55 * self.hasard.f(), y: 0.3 * self.hasard.f(), nee: temps });
        }
        let Some((x, y, nee)) = self.etoile.as_ref().map(|e| (e.x, e.y, e.nee)) else { return };
        let age = temps - nee;
        if age > VIE_DE_L_ETOILE {
            self.etoile = None;
            return;
        }
        let tete = rect.min + vec2(x * rect.width(), y * rect.height()) + VITESSE_DE_L_ETOILE * age as f32;
        let force = 1.0 - (age / VIE_DE_L_ETOILE) as f32;
        p.line_segment([tete, tete + TRAINE_DE_L_ETOILE], Stroke::new(1.8, voile_de(ETOILE, force)));
    }

    /// Les confettis d'un anniversaire (cdc:783-788) : des rectangles deux fois plus larges que hauts, à 75 %, qui
    /// tombent en tournant et en se balançant ; la moitié de la couleur de la personne, l'autre d'un blanc chaud.
    fn peindre_les_confettis(&mut self, p: &Painter, rect: Rect, temps: f64, dt: f32, couleur: Rgb) {
        let (l, h) = (rect.width(), rect.height());
        for q in &mut self.confettis {
            q.y += q.v * CHUTE_DES_CONFETTIS * dt;
            if q.y > 1.03 {
                q.y = -0.03;
                q.x = self.hasard.f();
            }
            let ph = f64::from(q.ph);
            let centre = pos2(rect.left() + q.x * l + (20.0 * (temps * 1.3 + ph).sin()) as f32, rect.top() + q.y * h);
            let (s, c) = ((temps * 2.0 + ph).rem_euclid(TAU) as f32).sin_cos();
            let (long, large) = (vec2(c, s) * (q.w / 2.0), vec2(-s, c) * (q.w / 4.0));
            let coins =
                vec![centre - long - large, centre + long - large, centre + long + large, centre - long + large];
            let teinte = if q.c < 0.5 { couleur } else { [255, 255, 255 - (q.c * 200.0) as u8] };
            p.add(Shape::convex_polygon(coins, voile_de(teinte, 0.75), Stroke::NONE));
        }
    }

    /// Le balayage (cdc:41, un dégradé de 3 px qui se répète, de bas en haut) : dans chaque bande de trois points, deux
    /// points de rien, puis un qui va de rien à 2 % de cyan en remontant. Le maillage se tire une fois par taille du
    /// centre.
    fn peindre_le_balayage(&mut self, p: &Painter, rect: Rect) {
        if self.balayage.as_ref().is_none_or(|(r, _)| *r != rect) {
            let ligne = voile_de(CYAN, 0.02);
            let mut m = Mesh::default();
            let mut bas = rect.bottom() - 2.0;
            while bas > rect.top() {
                let haut = (bas - 1.0).max(rect.top());
                let k = m.vertices.len() as u32;
                m.colored_vertex(pos2(rect.left(), bas), Color32::TRANSPARENT);
                m.colored_vertex(pos2(rect.right(), bas), Color32::TRANSPARENT);
                m.colored_vertex(pos2(rect.right(), haut), ligne);
                m.colored_vertex(pos2(rect.left(), haut), ligne);
                m.add_triangle(k, k + 1, k + 2);
                m.add_triangle(k, k + 2, k + 3);
                bas -= 3.0;
            }
            self.balayage = Some((rect, Arc::new(m)));
        }
        if let Some((_, m)) = &self.balayage {
            p.add(Shape::Mesh(m.clone()));
        }
    }

    /// L'écran de veille (cdc:50-53, 86, 577-580), centré à 120 points du haut : la grande horloge (64 points, espacée
    /// de 0,2 em, presque blanche), la date (15 points, 0,3 em, le bleu de l'horloge du tableau), la météo (15 points,
    /// 0,2 em, ambre), la prochaine échéance (13 points, 0,14 em, verte), aux marges de la page (2, 14 et 6 points),
    /// chaque ligne haute de 1,35 fois sa taille ; dans la lueur cyan de `text-shadow: 0 0 18px`. Une ligne vide ne
    /// prend pas de place, et ses marges se confondent avec la suivante, comme en CSS ; une ligne trop longue passe à
    /// la ligne, centrée.
    fn peindre_la_veille(&mut self, p: &Painter, rect: Rect, opacite: f32, contenu: &Veille) {
        let p = p.with_clip_rect(rect);
        let lignes = [
            (contenu.heure, 64.0, 0.2, BLANC, 0.0),
            (contenu.date, 15.0, 0.3, BLEU, 2.0),
            (contenu.meteo, 15.0, 0.2, hud::AMBRE, 14.0),
            (contenu.echeance, 13.0, 0.14, hud::VERT, 6.0),
        ];
        let largeur = (rect.width() - 40.0).max(120.0);
        let mut y = rect.top() + HAUT_DE_VEILLE;
        let mut marge: f32 = 0.0;
        for (rang, (texte, taille, em, couleur, avant)) in lignes.into_iter().enumerate() {
            marge = marge.max(avant);
            if texte.trim().is_empty() {
                continue;
            }
            y += marge;
            marge = 0.0;
            let espace = em * taille;
            let format = TextFormat {
                font_id: FontId::monospace(taille),
                color: couleur,
                extra_letter_spacing: espace,
                line_height: Some(taille * 1.35),
                ..Default::default()
            };
            let mut job = LayoutJob::single_section(texte.to_string(), format);
            job.wrap.max_width = largeur;
            job.halign = Align::Center;
            let galley = p.layout_job(job);
            // CSS espace aussi la dernière lettre : un texte centré glisse d'un demi-espacement vers la gauche.
            let pos = pos2(rect.center().x - espace / 2.0, y);
            if taille >= GRAND {
                // Chaque copie porte sa part de la lueur, arrondie de sorte que leur somme soit juste.
                let n = self.copies.len() as f32;
                let part = |k: usize, total: f32| {
                    ((k as f32 + 1.0) * total / n).round() as u8 - (k as f32 * total / n).round() as u8
                };
                let (vert, bleu) = (f32::from(CYAN[1]) * opacite, f32::from(CYAN[2]) * opacite);
                for (k, d) in self.copies.iter().enumerate() {
                    let lueur = Color32::from_rgb_additive(0, part(k, vert), part(k, bleu));
                    let mut copie = TextShape::new(pos + *d, galley.clone(), lueur);
                    copie.override_text_color = Some(lueur);
                    p.add(copie);
                }
            } else {
                // Le flou de chaque mot : la bande de ses capitales, à la part d'encre que lui laisse son espacement ;
                // tiré une fois pour ce texte à cette place, puis atténué le temps d'un fondu.
                let lueur = &mut self.lueurs[rang];
                if lueur.as_ref().is_none_or(|l| l.texte != texte || l.place != pos) {
                    let encre = ENCRE_D_UNE_CAPITALE / ((CHASSE + em) * CAPITALES);
                    let mut m = Mesh::default();
                    for rangee in &galley.rows {
                        for mot in mots(&rangee.glyphs, taille) {
                            lueur_floue(&mut m, mot.translate(pos.to_vec2() + rangee.pos.to_vec2()), encre);
                        }
                    }
                    *lueur = Some(LueurDeLigne { texte: texte.to_string(), place: pos, maillage: Arc::new(m) });
                }
                if let Some(l) = lueur {
                    if opacite < 1.0 {
                        let mut m = (*l.maillage).clone();
                        for s in &mut m.vertices {
                            s.color = s.color.gamma_multiply(opacite);
                        }
                        p.add(Shape::mesh(m));
                    } else {
                        p.add(Shape::Mesh(l.maillage.clone()));
                    }
                }
            }
            y += galley.size().y;
            let mut ecrit = TextShape::new(pos, galley, couleur);
            ecrit.opacity_factor = opacite;
            p.add(ecrit);
        }
    }
}

// ---- Les fondus, les lueurs, le hasard ----------------------------------------------------------------------------

/// Un fondu à la manière d'une transition CSS (`transition: opacity …`) : de la valeur qu'il avait quand sa cible a
/// changé vers la nouvelle, selon la courbe `ease`, en un temps proportionnel au chemin qui reste — un fondu interrompu
/// repart d'où il en est, plus court, comme dans le navigateur.
#[derive(Clone, Copy, Debug)]
struct Fondu {
    de: f32,
    vers: f32,
    depuis: f64,
    duree: f64,
}

impl Fondu {
    fn fixe(valeur: f32) -> Fondu {
        Fondu { de: valeur, vers: valeur, depuis: f64::NEG_INFINITY, duree: 0.0 }
    }

    fn valeur(&self, t: f64) -> f32 {
        if !self.bouge(t) {
            return self.vers;
        }
        self.de + (self.vers - self.de) * ease((t - self.depuis) / self.duree) as f32
    }

    /// Vers 0 ou 1, en `duree` secondes pour le chemin entier.
    fn viser(&mut self, cible: f32, t: f64, duree: f64) {
        if cible != self.vers {
            let de = self.valeur(t);
            *self = Fondu { de, vers: cible, depuis: t, duree: duree * f64::from((cible - de).abs()) };
        }
    }

    fn bouge(&self, t: f64) -> bool {
        t < self.depuis + self.duree
    }
}

/// La courbe `ease` des transitions CSS, `cubic-bezier(0.25, 0.1, 0.25, 1)` : la part faite d'un fondu à la part `u` de
/// son temps. x(s) = u se résout par dichotomie (x croît avec s), à 10⁻⁷ près.
fn ease(u: f64) -> f64 {
    let bezier = |s: f64, a: f64, b: f64| 3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s;
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let (mut bas, mut haut) = (0.0, 1.0);
    for _ in 0..24 {
        let milieu = (bas + haut) / 2.0;
        if bezier(milieu, 0.25, 0.25) < u {
            bas = milieu;
        } else {
            haut = milieu;
        }
    }
    bezier((bas + haut) / 2.0, 0.1, 1.0)
}

/// Les décalages des copies qui font la lueur de l'horloge : une gaussienne d'écart type [`SIGMA`] échantillonnée en
/// tournesol — des rayons selon la loi de Rayleigh, l'angle d'or d'une copie à l'autre —, chaque copie portant une part
/// égale de la lueur.
fn copies_de_la_lueur() -> Vec<Vec2> {
    (0..COPIES)
        .map(|k| {
            let u = (k as f32 + 0.5) / COPIES as f32;
            Vec2::angled(k as f32 * 2.399_963) * SIGMA * (-2.0 * (1.0 - u).ln()).sqrt()
        })
        .collect()
}

/// Les mots d'une rangée de texte : pour chacun, la bande de ses capitales — de la gauche de sa première lettre à la
/// droite de sa dernière, de la hauteur des capitales à la ligne de base —, dans les coordonnées de la rangée.
fn mots(glyphes: &[Glyph], taille: f32) -> Vec<Rect> {
    let mut mots = Vec::new();
    let mut mot: Option<Rect> = None;
    for g in glyphes {
        if g.chr.is_whitespace() || g.uv_rect.is_nothing() {
            mots.extend(mot.take());
            continue;
        }
        let gauche = g.pos.x + g.uv_rect.offset.x;
        let haut = g.pos.y - CAPITALES * taille;
        let lettre = Rect::from_min_max(pos2(gauche, haut), pos2(gauche + g.uv_rect.size.x, g.pos.y));
        mot = Some(mot.map_or(lettre, |m| m.union(lettre)));
    }
    mots.extend(mot);
    mots
}

/// La lueur d'un mot de petit texte, ajoutée au maillage `m` : la bande de ses capitales (`boite`), pleine, floutée
/// d'une gaussienne d'écart type [`SIGMA`] — ce que l'ombre d'un `text-shadow` de 18 px fait de lettres de 13 ou 15
/// points, qu'elle fond ensemble —, en cyan additif, `force` (sa part d'encre) en son cœur. Le flou d'un rectangle est
/// le produit de deux profils, Φ((x − a)/σ) − Φ((x − b)/σ) : une grille de sommets, plus serrée là où il varie, entre
/// lesquels la carte graphique interpole.
fn lueur_floue(m: &mut Mesh, boite: Rect, force: f32) {
    const PAS: [f32; 7] = [-2.5, -1.5, -0.75, 0.0, 0.75, 1.5, 2.5];
    let arrets = |a: f32, b: f32| {
        let mut v: Vec<f32> = PAS.iter().flat_map(|k| [a + k * SIGMA, b + k * SIGMA]).collect();
        v.sort_by(f32::total_cmp);
        v.dedup_by(|x, y| (*x - *y).abs() < 0.5);
        v
    };
    let profil = |x: f32, a: f32, b: f32| phi((x - a) / SIGMA) - phi((x - b) / SIGMA);
    let (xs, ys) = (arrets(boite.left(), boite.right()), arrets(boite.top(), boite.bottom()));
    let debut = m.vertices.len() as u32;
    for &y in &ys {
        let fy = force * profil(y, boite.top(), boite.bottom());
        for &x in &xs {
            m.colored_vertex(pos2(x, y), lumiere(CYAN, fy * profil(x, boite.left(), boite.right())));
        }
    }
    let n = xs.len() as u32;
    for j in 0..ys.len() as u32 - 1 {
        for i in 0..n - 1 {
            let k = debut + j * n + i;
            m.add_triangle(k, k + 1, k + n + 1);
            m.add_triangle(k, k + n + 1, k + n);
        }
    }
}

/// La fonction de répartition de la loi normale : Φ(x) = (1 + erf(x/√2))/2, erf d'Abramowitz et Stegun (7.1.26), à
/// 1,5 × 10⁻⁷ près.
fn phi(x: f32) -> f32 {
    let z = x.abs() / std::f32::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let polynome = t * (0.254_829_6 + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let erf = 1.0 - polynome * (-z * z).exp();
    if x >= 0.0 { 0.5 * (1.0 + erf) } else { 0.5 * (1.0 - erf) }
}

/// Une couleur qui s'ajoute à ce qu'elle recouvre (le `lighter` du canevas) : les lueurs s'additionnent.
fn lumiere(c: Rgb, force: f32) -> Color32 {
    let f = force.clamp(0.0, 1.0);
    let x = |v: u8| (f32::from(v) * f).round() as u8;
    Color32::from_rgb_additive(x(c[0]), x(c[1]), x(c[2]))
}

/// Une couleur ordinaire, plus ou moins transparente (le `rgba(…)` de la page).
fn voile_de(c: Rgb, opacite: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (opacite.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Le hasard des particules (`Math.random` dans la page) : SplitMix64, comme l'orbe et la musique.
struct Hasard(u64);

impl Hasard {
    /// Uniforme dans [0, 1).
    fn suivant(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn f(&mut self) -> f32 {
        self.suivant() as f32
    }

    fn entre(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

// ---- Les dates ----------------------------------------------------------------------------------------------------

/// « 2026-10-24 » → (2026, 10, 24) ; `None` pour une date mal formée.
fn jour_de(date: &str) -> Option<(i32, u32, u32)> {
    let mut p = date.trim().split('-');
    let a: i32 = p.next()?.parse().ok()?;
    let m: u32 = p.next()?.parse().ok()?;
    let j: u32 = p.next()?.get(..2)?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&j)).then_some((a, m, j))
}

/// « MM-JJ » ou « AAAA-MM-JJ » → (mois, jour).
fn anniversaire(texte: &str) -> Option<(u32, u32)> {
    let parties: Vec<&str> = texte.trim().split('-').collect();
    let (m, j) = match parties.as_slice() {
        [m, j] | [_, m, j] => (m.parse::<u32>().ok()?, j.parse::<u32>().ok()?),
        _ => return None,
    };
    ((1..=12).contains(&m) && (1..=31).contains(&j)).then_some((m, j))
}

/// Le jour de la semaine (0 : dimanche), par la méthode de Sakamoto.
fn jour_de_la_semaine(annee: i32, mois: u32, jour: u32) -> usize {
    const DECALAGES: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let a = if mois < 3 { annee - 1 } else { annee };
    (a + a.div_euclid(4) - a.div_euclid(100) + a.div_euclid(400) + DECALAGES[mois as usize - 1] + jour as i32)
        .rem_euclid(7) as usize
}

/// Une couleur d'`EVE_FAMILLE` : `[r, v, b]` ou « #rrvvbb ».
fn rgb_de(v: &Value) -> Option<Rgb> {
    if let Some(t) = v.as_str() {
        let h = t.trim().strip_prefix('#')?;
        let octet = |k: usize| u8::from_str_radix(h.get(k..k + 2)?, 16).ok();
        return (h.len() == 6).then(|| Some([octet(0)?, octet(2)?, octet(4)?])).flatten();
    }
    let a = v.as_array()?;
    let c = |k: usize| a.get(k)?.as_f64().filter(|x| (0.0..=255.0).contains(x)).map(|x| x.round() as u8);
    (a.len() == 3).then(|| Some([c(0)?, c(1)?, c(2)?])).flatten()
}

#[cfg(test)]
mod essais {
    use super::*;
    use eframe::egui;
    use serde_json::json;

    /// Une famille d'exemple (personne de vrai).
    fn famille() -> Value {
        json!({ "personnes": [
            { "nom": "Hugo", "anniversaire": "03-07" },
            { "nom": "Zoé", "couleur": [255, 122, 184], "anniversaire": "2019-09-15" },
            { "nom": "  ", "anniversaire": "05-05" },
            { "nom": "Iris", "couleur": "#5aa9ff", "anniversaire": "2016-02-29" },
            { "nom": "Théo", "couleur": [300, 0, 0], "anniversaire": "12-31" },
            { "nom": "Sans date" },
        ] })
    }

    #[test]
    fn les_saisons_du_tableau() {
        use Saison::*;
        for (date, attendu) in [
            ("2026-10-19", None),
            ("2026-10-20", Some(Halloween)),
            ("2026-10-31", Some(Halloween)),
            ("2026-11-01", Some(Halloween)),
            ("2026-11-02", None),
            ("2026-12-01", Some(Neige)),
            ("2026-12-17", Some(Neige)),
            ("2026-12-18", Some(Noel)),
            ("2026-12-26", Some(Noel)),
            ("2026-12-27", Some(Neige)),
            ("2027-01-15", Some(Neige)),
            ("2027-03-31", Some(Neige)),
            ("2027-04-01", None),
            ("2027-06-15", None),
        ] {
            assert_eq!(saison(date, false), attendu, "{date}");
        }
        assert_eq!(saison("2027-06-15", true), Some(Neige), "il neige en juin : la neige");
        assert_eq!(saison("2026-10-24", true), Some(Halloween), "l'Halloween passe devant la neige");
        assert_eq!(saison("2026-12-24", true), Some(Noel));
        assert_eq!(saison("pas une date", true), Some(Neige));
        assert_eq!(saison("pas une date", false), None);
    }

    #[test]
    fn la_nuit_va_de_21_h_a_6_h() {
        let nuits: Vec<u32> = (0..24).filter(|h| est_la_nuit(*h)).collect();
        assert_eq!(nuits, [0, 1, 2, 3, 4, 5, 21, 22, 23]);
    }

    #[test]
    fn la_fete_du_jour() {
        let f = famille();
        let fete = |date: &str| fete_du_jour(&f, date);
        assert_eq!(fete("2026-09-15"), Some(Fete { nom: "Zoé".into(), couleur: [255, 122, 184] }));
        assert_eq!(
            fete("2027-03-07"),
            Some(Fete { nom: "Hugo".into(), couleur: QUARTIERS[0] }),
            "sans couleur, celle de son quartier"
        );
        assert_eq!(fete("2026-05-05"), None, "sans nom, pas de fête");
        assert_eq!(fete("2028-02-29").map(|x| x.couleur), Some([0x5a, 0xa9, 0xff]), "une couleur « #rrvvbb »");
        assert_eq!(fete("2027-02-28").map(|x| x.nom), Some("Iris".into()), "le 28 février des années communes");
        assert_eq!(fete("2028-02-28"), None, "mais pas les années bissextiles");
        assert_eq!(fete("2026-12-31").map(|x| x.couleur), Some(QUARTIERS[0]), "une couleur impossible : le quartier");
        assert_eq!(fete("2026-10-24"), None);
        assert_eq!(fete_du_jour(&Value::Null, "2026-09-15"), None, "pas de famille, pas de fête");
        assert_eq!(fete_du_jour(&json!({ "personnes": "Zoé" }), "2026-09-15"), None);
    }

    #[test]
    fn les_mots_de_la_veille() {
        assert_eq!(date_longue("2026-10-24"), "SAMEDI 24 OCTOBRE");
        assert_eq!(date_longue("2027-02-01"), "LUNDI 1 FÉVRIER");
        assert_eq!(date_longue("2026-08-15"), "SAMEDI 15 AOÛT");
        assert_eq!(date_longue("2028-02-29"), "MARDI 29 FÉVRIER");
        assert_eq!(date_longue("2000-01-01"), "SAMEDI 1 JANVIER");
        assert_eq!(date_longue("demain"), "demain");
        assert_eq!(heure_de_veille("2026-10-24 09-05-00"), "09 h 05");
        assert_eq!(heure_de_veille("2026-10-24 21-47-13"), "21 h 47");
        assert_eq!(heure_de_veille("2026-10-24"), "");
        assert_eq!(prochaine_echeance(0, "Bulletin"), "PROCHAINE ÉCHÉANCE · AUJOURD'HUI · BULLETIN");
        assert_eq!(prochaine_echeance(1, "pneus d'hiver"), "PROCHAINE ÉCHÉANCE · DEMAIN · PNEUS D'HIVER");
        assert_eq!(prochaine_echeance(7, "Réunion · école"), "PROCHAINE ÉCHÉANCE · DANS 7 J · RÉUNION · ÉCOLE");
        assert_eq!(souhait_a("Zoé"), "JOYEUX ANNIVERSAIRE, ZOÉ!");
    }

    #[test]
    fn les_fondus_sont_ceux_du_navigateur() {
        assert_eq!((ease(0.0), ease(1.0)), (0.0, 1.0));
        // cubic-bezier(.25, .1, .25, 1) au quart et à la moitié du temps : 0,4085 et 0,8024.
        assert!((ease(0.25) - 0.408_5).abs() < 1e-3, "{}", ease(0.25));
        assert!((ease(0.5) - 0.802_4).abs() < 1e-3, "{}", ease(0.5));
        let mut f = Fondu::fixe(0.0);
        assert_eq!(f.valeur(5.0), 0.0);
        f.viser(1.0, 10.0, 2.5);
        assert!(f.bouge(11.0) && !f.bouge(12.6));
        assert!(f.valeur(11.25) > 0.5 && f.valeur(11.25) < 1.0);
        assert_eq!(f.valeur(12.5), 1.0);
        // Interrompu en chemin, il repart d'où il en est, dans le temps qu'il faut pour ce chemin-là.
        let mut g = Fondu::fixe(0.0);
        g.viser(1.0, 0.0, 2.5);
        let v = g.valeur(1.0);
        g.viser(0.0, 1.0, 2.5);
        assert!((g.valeur(1.0) - v).abs() < 1e-6, "sans saut");
        assert!((g.duree - 2.5 * f64::from(v)).abs() < 1e-6, "un retour plus court");
        assert_eq!(g.valeur(1.0 + g.duree), 0.0);
        assert!((phi(0.0) - 0.5).abs() < 1e-6 && (phi(1.959_964) - 0.975).abs() < 1e-5 && phi(-8.0) < 1e-6);
    }

    #[test]
    fn les_couleurs_d_eve_famille() {
        assert_eq!(rgb_de(&json!([0, 229, 255])), Some([0, 229, 255]));
        assert_eq!(rgb_de(&json!("#FFB347")), Some([255, 179, 71]));
        assert_eq!(rgb_de(&json!("#ffb34")), None);
        assert_eq!(rgb_de(&json!([0, 229])), None);
        assert_eq!(rgb_de(&json!([-1, 0, 0])), None);
        assert_eq!(rgb_de(&json!("rouge")), None);
    }

    /// Une image sans fenêtre, sur un centre de 1200 × 800 : ce que rend `dessiner`, l'opacité de la veille, et les
    /// sommets dessinés (tous finis).
    fn une_image(
        ctx: &egui::Context,
        a: &mut Ambiance,
        t: f64,
        moment: &Moment,
        bloquee: bool,
        evenements: Vec<Event>,
    ) -> (bool, f32, usize) {
        let entree = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            time: Some(t),
            events: evenements,
            ..Default::default()
        };
        let (mut bouge, mut opacite) = (false, 0.0);
        let contenu = Veille {
            heure: "14 h 05",
            date: "SAMEDI 24 OCTOBRE",
            meteo: "VILLE D'ESSAI · 4 °C · COUVERT",
            echeance: "PROCHAINE ÉCHÉANCE · DANS 3 J · RAPPORT D'ESSAI",
        };
        let mut sortie = ctx.run_ui(entree, |ui| {
            let rect = ui.max_rect();
            bouge = a.dessiner(ui.painter(), rect, t, moment);
            opacite = a.veille(ui.painter(), rect, t, bloquee, &contenu);
        });
        sortie.textures_delta.clear();
        let mut sommets = 0;
        for p in ctx.tessellate(std::mem::take(&mut sortie.shapes), sortie.pixels_per_point) {
            if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                assert!(m.vertices.iter().all(|v| v.pos.x.is_finite() && v.pos.y.is_finite()), "à {t} s");
                sommets += m.vertices.len();
            }
        }
        (bouge, opacite, sommets)
    }

    fn moment<'a>(date: &'a str, heure: u32, fete: Option<&'a Fete>) -> Moment<'a> {
        Moment { date, heure, econome: false, il_neige: false, fete }
    }

    #[test]
    fn l_ambiance_se_dessine_sans_fenetre() {
        let ctx = egui::Context::default();
        let mut a = Ambiance::nouvelle();
        // Un jour sans saison : le balayage seul, rien ne bouge.
        let (bouge, _, balayage) = une_image(&ctx, &mut a, 0.0, &moment("2027-06-15", 14, None), false, vec![]);
        assert!(!bouge && balayage > 1000, "{bouge} {balayage}");
        // L'Halloween : ses braises bougent, et montent.
        let y0: f32 = a.particules.iter().map(|q| q.y).sum();
        let (bouge, _, halloween) = une_image(&ctx, &mut a, 0.1, &moment("2026-10-24", 14, None), false, vec![]);
        let y1: f32 = a.particules.iter().map(|q| q.y).sum();
        assert!(bouge && halloween > balayage && y1 < y0, "{halloween} {y0} {y1}");
        // La neige tombe ; en mode économe, ni saison ni confettis.
        let (bouge, _, _) = une_image(&ctx, &mut a, 0.2, &moment("2027-01-15", 14, None), false, vec![]);
        assert!(bouge && a.particules.iter().map(|q| q.y).sum::<f32>() > y1);
        let econome = Moment { econome: true, ..moment("2026-10-24", 14, None) };
        let (bouge, _, sommets) = une_image(&ctx, &mut a, 0.3, &econome, false, vec![]);
        assert!(!bouge && sommets == balayage, "{sommets}");
        // La fête : ses confettis, et tout à la fois — le budget d'une image reste petit.
        let zoe = Fete { nom: "Zoé".into(), couleur: [255, 122, 184] };
        let (bouge, _, tout) = une_image(&ctx, &mut a, 0.4, &moment("2026-10-24", 14, Some(&zoe)), false, vec![]);
        assert!(bouge && tout > halloween && tout < 8_000, "{tout}");
    }

    #[test]
    fn la_nuit_vient_en_quatre_secondes() {
        let ctx = egui::Context::default();
        let mut a = Ambiance::nouvelle();
        let jour = moment("2027-06-15", 14, None);
        let nuit = moment("2027-06-15", 21, None);
        // Ouverte la nuit, la fenêtre y est d'emblée.
        let mut b = Ambiance::nouvelle();
        assert!(!une_image(&ctx, &mut b, 0.0, &nuit, false, vec![]).0);
        assert_eq!(b.nuit.map(|n| n.valeur(0.0)), Some(1.0));
        assert!(!une_image(&ctx, &mut a, 0.0, &jour, false, vec![]).0);
        assert!(une_image(&ctx, &mut a, 1.0, &nuit, false, vec![]).0, "le voile vient");
        assert!(une_image(&ctx, &mut a, 4.9, &nuit, false, vec![]).0);
        assert!(!une_image(&ctx, &mut a, 5.1, &nuit, false, vec![]).0, "en 4 s");
        assert_eq!(a.nuit.map(|n| n.valeur(5.1)), Some(1.0));
        assert!(une_image(&ctx, &mut a, 6.0, &jour, false, vec![]).0, "et s'en va");
        assert!(!une_image(&ctx, &mut a, 10.1, &jour, false, vec![]).0);
    }

    #[test]
    fn la_veille_vient_apres_90_s_et_la_souris_la_chasse() {
        let ctx = egui::Context::default();
        let mut a = Ambiance::nouvelle();
        let m = moment("2027-06-15", 14, None);
        let image = |a: &mut Ambiance, t: f64, bloquee: bool, ev: Vec<Event>| une_image(&ctx, a, t, &m, bloquee, ev);
        // Des images chaque seconde, de `de` à `a` (exclu) : l'opacité la plus haute de la veille.
        let attendre = |a: &mut Ambiance, de: u32, jusqu_a: u32, bloquee: bool| {
            (de..jusqu_a).map(|t| image(a, f64::from(t), bloquee, vec![]).1).fold(0.0f32, f32::max)
        };
        assert_eq!(attendre(&mut a, 0, 90, false), 0.0, "rien pendant 90 s");
        let (bouge, o, sans_veille) = image(&mut a, 90.0, false, vec![]);
        assert!(o == 0.0 && !bouge, "à 90 s, le fondu commence");
        let (bouge, o, _) = image(&mut a, 91.25, false, vec![]);
        assert!(bouge && o > 0.5 && o < 1.0, "{o}");
        let (_, o, avec_veille) = image(&mut a, 92.6, false, vec![]);
        assert_eq!(o, 1.0);
        assert!(avec_veille > sans_veille + 200, "{sans_veille} {avec_veille}");
        // La souris qui reste en place n'est pas un geste ; qui bouge, elle chasse la veille.
        let ici = Event::PointerMoved(pos2(300.0, 300.0));
        assert_eq!(image(&mut a, 93.0, false, vec![ici.clone()]).1, 1.0);
        assert_eq!(image(&mut a, 93.5, false, vec![ici]).1, 1.0);
        assert_eq!(image(&mut a, 94.0, false, vec![Event::PointerMoved(pos2(310.0, 300.0))]).1, 1.0, "elle part de là");
        let (bouge, o, _) = image(&mut a, 95.0, false, vec![]);
        assert!(bouge && o > 0.0 && o < 1.0, "{o}");
        assert_eq!(image(&mut a, 96.6, false, vec![]).1, 0.0, "partie en 2,5 s");
        // Un outil en cours la retient ; fini, elle vient aussitôt, comme dans la page.
        assert_eq!(attendre(&mut a, 97, 200, true), 0.0);
        image(&mut a, 200.0, false, vec![]);
        assert!(image(&mut a, 201.0, false, vec![]).1 > 0.0);
        // Une touche, un signal la repoussent de 90 s.
        let touche = Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        image(&mut a, 202.0, false, vec![touche]);
        assert!(image(&mut a, 203.0, false, vec![]).1 < 0.5, "la touche la chasse");
        image(&mut a, 204.0, false, vec![]);
        assert_eq!(attendre(&mut a, 205, 291, false), 0.0);
        a.signal(290.5);
        assert_eq!(attendre(&mut a, 291, 381, false), 0.0);
        assert_eq!(image(&mut a, 381.0, false, vec![]).1, 0.0, "90 s après le signal, le fondu commence");
        assert!(image(&mut a, 382.0, false, vec![]).1 > 0.0);
        // Le centre replié longtemps : y revenir est une activité, la veille ne surgit pas.
        let mut b = Ambiance::nouvelle();
        image(&mut b, 0.0, false, vec![]);
        assert_eq!(image(&mut b, 300.0, false, vec![]).1, 0.0);
        assert_eq!(image(&mut b, 301.0, false, vec![]).1, 0.0);
    }

    #[test]
    fn la_lueur_d_une_petite_ligne_suit_ses_mots() {
        let ctx = egui::Context::default();
        let mut bandes = Vec::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let format =
                TextFormat { font_id: FontId::monospace(15.0), extra_letter_spacing: 4.5, ..Default::default() };
            let galley = ui.painter().layout_job(LayoutJob::single_section("SAMEDI  24 OCTOBRE ".into(), format));
            bandes = mots(&galley.rows[0].glyphs, 15.0);
        });
        assert_eq!(bandes.len(), 3, "{bandes:?}");
        assert!(bandes.windows(2).all(|b| b[0].right() < b[1].left()), "dans l'ordre, sans se toucher");
        for b in &bandes {
            assert!((b.height() - CAPITALES * 15.0).abs() < 1e-3, "la hauteur des capitales");
        }
        assert!(bandes[0].width() > bandes[1].width() * 2.0, "SAMEDI est plus long que 24");
        // Pleine, la lueur d'un mot est la plus forte en son milieu ; elle s'éteint à deux écarts types et demi de ses
        // bords.
        let profil = |x: f32| phi(x / SIGMA) - phi((x - 60.0) / SIGMA);
        assert!(profil(30.0) > 0.99 && (profil(0.0) - 0.5).abs() < 1e-3 && profil(-2.5 * SIGMA) < 0.01);
    }

    #[test]
    fn le_souhait_se_dit_une_fois() {
        let ctx = egui::Context::default();
        let mut a = Ambiance::nouvelle();
        let zoe = Fete { nom: "Zoé".into(), couleur: [255, 122, 184] };
        une_image(&ctx, &mut a, 10.0, &moment("2026-09-15", 9, Some(&zoe)), false, vec![]);
        assert_eq!(a.souhait(11.0), None, "pas encore");
        assert_eq!(a.souhait(12.5).as_deref(), Some("JOYEUX ANNIVERSAIRE, ZOÉ!"));
        assert_eq!(a.souhait(13.0), None, "une fois");
        une_image(&ctx, &mut a, 20.0, &moment("2026-09-15", 9, Some(&zoe)), false, vec![]);
        assert_eq!(a.souhait(30.0), None, "la même fête ne se resouhaite pas");
        une_image(&ctx, &mut a, 40.0, &moment("2026-09-16", 9, None), false, vec![]);
        assert_eq!(a.souhait(50.0), None);
    }
}
