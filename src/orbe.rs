//! L'orbe d'Eve, dans la fenêtre native : celle de l'Eve de Constructo AI (sa page, `jarvis.html`,
//! `const Orbe`), portée de WebGL2 à OpenGL — de la lumière vivante dans une sphère de verre.
//!
//! Des fils de lumière tournent sur des orbites qui précessent autour d'un cœur qui respire : chaque fil est une courbe
//! 3D de 33 points, un peu ondulée, projetée avec de la perspective, et sa lueur vient de la distance exacte de chaque
//! pixel à la courbe — fine et vive devant, plus pâle et plus floue au fond ; la tête brille, la queue s'éteint. Le
//! cœur s'éteint en 1/d², blanc au centre puis chaud ; une poussière flotte et s'éclaire près de lui. Trois temps par
//! image, sur la carte graphique : la lumière, son éclat (un flou à plusieurs échelles, le « bloom » d'une photo), puis
//! le verre — l'intérieur vu à travers lui (réfracté vers les bords, un peu dispersé en couleurs), le studio qu'il
//! renvoie selon Fresnel, son liseré, un point brillant, l'ombre et la lumière qu'il pose dessous — et un tone mapping
//! ACES. Le processeur ne fait que préparer les courbes : [`Vie`], du Rust pur, essayé plus bas.
//!
//! Elle vit : elle remarque la souris (une onde part de son cœur, ses fils s'en approchent), la suit du regard, regarde
//! la carte quand elle attend un accord, réagit au toucher (un élan), somnole après quatre minutes de calme et se
//! réveille à la moindre chose ; au repos, une pensée la traverse de temps en temps (une impulsion court le long d'un
//! fil) ; quand elle réfléchit, les impulsions se suivent ; pendant la transcription, elle s'allège pour laisser le
//! processeur à Whisper (15 images par seconde, un fil sur deux, sans éclat).
//!
//! Le portage (`docs\fenetre-native.md`, § 3.6) : un rappel d'egui_glow, où les programmes, les textures et les
//! surfaces se créent une fois, sur le fil du rendu. Ses trois pièges : egui_glow ne relie pas son framebuffer après
//! un rappel — l'orbe relie elle-même, avant sa dernière passe, celui qu'elle a trouvé ; dans ce framebuffer, le centre
//! de la sphère se décale de l'origine du carré (`gl_FragCoord` compte depuis le coin de la fenêtre) ; le verre s'y
//! mélange en (ONE, ONE_MINUS_SRC_ALPHA), l'alpha prémultiplié de la page : hors de la sphère, la lumière s'ajoute au
//! fond et l'ombre l'assombrit. Si OpenGL refuse quoi que ce soit, l'orbe ne fait jamais tomber la fenêtre : un disque
//! peint par egui la remplace, et le journal dit pourquoi.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, Pos2, Rect};
use eframe::egui_glow::{self, ShaderVersion};
use eframe::glow::{self, HasContext as _};

use crate::journal;

const TOUR: f64 = std::f64::consts::TAU;
/// 44 fils de 33 points : 32 segments, en 4 morceaux de 8 (un morceau loin du pixel n'est pas calculé).
const N: usize = 44;
const POINTS: usize = 33;
const MORCEAUX: usize = 4;
const SEG: usize = 8;
/// Chaque fil occupe une ligne de la texture des données : ses points (x, y, intensité, 1/largeur²), sa couleur, sa
/// tête, les boîtes de ses morceaux.
const C_COULEUR: usize = POINTS;
const C_TETE: usize = POINTS + 1;
const C_BOITES: usize = POINTS + 2;
const LARGEUR: usize = POINTS + 2 + MORCEAUX;
/// En rayons de sphère : la portée de la lueur d'un fil, la réfraction du bord (on y voit l'intérieur 12 % plus près
/// du centre), la taille et la perspective de l'intérieur, la largeur d'un fil ; puis l'intensité des fils et du cœur.
const PORTEE: f64 = 0.085;
const LENTILLE: f64 = 0.12;
const ECHELLE: f64 = 0.9;
const PERSPECTIVE: f64 = 0.2;
const LARGE: f64 = 0.0085;
const INTENSITE: f64 = 2.8;
const ENERGIE: f64 = 3.4;
const ETINCELLES: usize = 46;
/// La sphère occupe 86 % de la demi-toile : le reste, autour, pour le halo, les étincelles et l'ombre.
const SPHERE: f64 = 0.86;
/// Le pas d'une image, plafonné comme dans la page (0,1 s) : une image en retard ne fait pas sauter l'orbe.
const PAS_MAX: f64 = 0.1;
/// Les cadences : quand elle s'anime, allégée (la transcription), au repos. Au repos, la page tournait à la cadence de
/// l'écran : à 4 images par seconde (le premier réglage), l'orbe et le réseau avançaient par saccades, et l'on y
/// voyait du « lag » (le 23 octobre). La carte graphique fait le dessin ; le processeur prépare chaque image.
const IMAGES: f64 = 30.0;
const IMAGES_ALLEGEES: f64 = 15.0;
const IMAGES_AU_REPOS: f64 = 30.0;
/// La somnolence : quatre minutes au repos.
const SOMMEIL: f64 = 240.0;
/// Sous cette taille (en points), un fil sur trois s'efface : l'écheveau reste lisible (la carte d'accord la réduit à
/// 112).
const PETITE: f32 = 170.0;

/// Ce qu'un état donne à voir (`ETATS` de la page) : la vitesse (le temps des orbites, de la précession, de la
/// poussière, des étincelles), l'agitation (l'ondulation des orbites, la respiration de leur rayon), l'éclat (la
/// lumière des fils et du cœur), le cœur et la teinte (en sRGB, de 0 à 255 : le liseré, le halo, le verre), le
/// mélange (la part de la teinte dans la couleur des fils).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Apparence {
    vitesse: f64,
    agitation: f64,
    eclat: f64,
    coeur: [f64; 3],
    teinte: [f64; 3],
    melange: f64,
}

const fn apparence(
    vitesse: f64,
    agitation: f64,
    eclat: f64,
    coeur: [f64; 3],
    teinte: [f64; 3],
    melange: f64,
) -> Apparence {
    Apparence { vitesse, agitation, eclat, coeur, teinte, melange }
}

/// Les huit états de la page (celle de Constructo AI : le bleu de l'ERP au repos et en parole, son ambre à l'accord),
/// sous les noms de l'état publié.
const ETATS: [(&str, Apparence); 8] = [
    ("demarrage", apparence(0.30, 0.25, 0.55, [214.0, 232.0, 255.0], [0.0, 120.0, 212.0], 0.0)),
    ("pret", apparence(0.42, 0.30, 0.80, [224.0, 238.0, 255.0], [40.0, 140.0, 230.0], 0.0)),
    ("ecoute", apparence(0.85, 0.80, 1.0, [255.0, 205.0, 185.0], [255.0, 110.0, 80.0], 0.35)),
    ("transcription", apparence(1.4, 0.55, 0.95, [205.0, 238.0, 255.0], [90.0, 205.0, 255.0], 0.35)),
    ("reflexion", apparence(2.1, 1.0, 1.0, [222.0, 228.0, 255.0], [120.0, 150.0, 255.0], 0.40)),
    ("parole", apparence(0.80, 0.65, 1.0, [206.0, 228.0, 255.0], [0.0, 120.0, 212.0], 0.30)),
    ("confirmation", apparence(0.55, 0.50, 0.95, [255.0, 214.0, 90.0], [245.0, 158.0, 11.0], 0.55)),
    ("erreur", apparence(0.14, 0.15, 0.35, [205.0, 125.0, 125.0], [200.0, 80.0, 80.0], 0.60)),
];

/// L'état et son apparence ; un nom inconnu vaut « pret ».
fn etat_connu(nom: &str) -> (&'static str, Apparence) {
    ETATS.iter().find(|(n, _)| *n == nom).copied().unwrap_or(ETATS[1])
}

/// La palette des fils : le jaune, le rose, le cyan, le bleu, l'orange, le violet, la menthe, la crème.
const PALETTE: [[f64; 3]; 8] = [
    [244.0, 194.0, 27.0],
    [255.0, 112.0, 190.0],
    [90.0, 212.0, 255.0],
    [111.0, 163.0, 255.0],
    [255.0, 150.0, 70.0],
    [176.0, 120.0, 255.0],
    [120.0, 255.0, 214.0],
    [255.0, 240.0, 205.0],
];

/// Une composante sRGB (0 à 255) en lumière linéaire : le calcul se fait en lumière, l'écran reçoit du sRGB.
fn lin(v: f64) -> f64 {
    let v = v / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn lineaire(c: [f64; 3]) -> [f64; 3] {
    c.map(lin)
}

/// Le tone mapping ACES de la scène (le même que le shader) : il garde le centre éclatant sans le brûler.
fn aces(x: f64) -> f64 {
    (x * (2.51 * x + 0.03) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

/// De la lumière linéaire (0 à 1) au sRGB (0 à 1), comme `versSrgb` dans le shader.
fn vers_srgb(c: f64) -> f64 {
    if c < 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Un souffle irrégulier : trois sinusoïdes de fréquences sans commune mesure.
fn souffle(x: f64) -> f64 {
    0.5 * x.sin() + 0.3 * (x * 2.17 + 1.3).sin() + 0.2 * (x * 4.31 + 0.7).sin()
}

fn glisser(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

/// Le hasard de l'orbe (`Math.random` dans la page) : SplitMix64, de quoi tirer un écheveau.
struct Hasard(u64);

impl Hasard {
    fn suivant(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn signe(&mut self) -> f64 {
        if self.suivant() < 0.5 { -1.0 } else { 1.0 }
    }
}

type V3 = [f64; 3];

fn croix(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unitaire(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let l = if l > 0.0 { l } else { 1.0 };
    [v[0] / l, v[1] / l, v[2] / l]
}

/// Tourne un vecteur autour de l'axe vertical (`ay`), puis de l'axe horizontal (`ax`).
fn tourner(v: V3, ay: f64, ax: f64) -> V3 {
    let (c, s) = (ay.cos(), ay.sin());
    let (x, z, y) = (v[0] * c + v[2] * s, v[2] * c - v[0] * s, v[1]);
    let (c, s) = (ax.cos(), ax.sin());
    [x, y * c - z * s, z * c + y * s]
}

/// Un fil de lumière : son orbite (un axe et le plan qui voyage avec lui, ce qui le fait précesser sans jamais
/// sauter), son rayon, sa vitesse, sa couleur, les lobes qui la font onduler et sortir de son plan, son arc de la queue
/// à la tête, son gain (quelques fils vifs, beaucoup de discrets : la hiérarchie d'une vraie photo de traînées).
struct Fil {
    ax: V3,
    e1: V3,
    e2: V3,
    r: f64,
    w: f64,
    phase: f64,
    prec: [f64; 2],
    c: V3,
    o: f64,
    lobes: f64,
    arc: f64,
    gain: f64,
    o2: f64,
    lobes2: f64,
}

impl Fil {
    fn tirer(i: usize, h: &mut Hasard) -> Fil {
        let u = h.suivant() * 2.0 - 1.0;
        let th = h.suivant() * TOUR;
        let s = (1.0 - u * u).sqrt();
        let ax = [s * th.cos(), s * th.sin(), u];
        let e1 = unitaire(croix(ax, if ax[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let r = 0.32 + 0.62 * h.suivant().powf(0.9);
        let w = (0.55 + h.suivant()) * h.signe();
        let phase = h.suivant() * TOUR;
        let prec = [0.12 + h.suivant() * 0.3, (h.suivant() - 0.5) * 0.25];
        let o = h.suivant() * TOUR;
        let lobes = 1.0 + (h.suivant() * 3.0).floor();
        let arc = 4.0 + h.suivant() * 2.2;
        let gain = 0.3 + 1.1 * h.suivant().powi(2);
        let o2 = h.suivant() * TOUR;
        let lobes2 = 1.0 + (h.suivant() * 2.0).floor();
        Fil {
            ax,
            e1,
            e2: croix(ax, e1),
            r,
            w,
            phase,
            prec,
            c: lineaire(PALETTE[i % PALETTE.len()]),
            o,
            lobes,
            arc,
            gain,
            o2,
            lobes2,
        }
    }
}

/// La vie de l'orbe (`vivant` dans la page) : la souris qu'elle remarque, son regard, le toucher, le sommeil.
struct Vivant {
    /// La souris, en rayons de sphère depuis son centre ; dedans (sous 1,05) ; le survol qui glisse vers 1 ou 0.
    hx: f64,
    hy: f64,
    dedans: bool,
    survol: f64,
    /// L'onde qui part du cœur (ou du point touché) et l'élan d'un toucher, qui s'éteignent.
    onde: f64,
    onde_x: f64,
    onde_y: f64,
    elan: f64,
    /// Le regard et sa cible ; l'instant du dernier mouvement de la souris ; le prochain coup d'œil.
    rx: f64,
    ry: f64,
    cible_rx: f64,
    cible_ry: f64,
    vu: f64,
    prochain_regard: f64,
    /// Le temps passé sans rien ; la veille (0 éveillée, 1 endormie).
    oisif: f64,
    veille: f64,
}

/// Ce que l'orbe montre de son état à l'instant (`vue` dans la page) : l'éclat, la vitesse, la vie (la voix, la
/// parole, la pensée).
struct Vue {
    eclat: f64,
    vitesse: f64,
    vie: f64,
}

/// Tout ce que calcule le processeur, d'une image à l'autre : l'écheveau, l'état qui glisse vers sa cible, la vie, et
/// les courbes de l'image posées dans la texture des données. Du Rust pur, sans OpenGL ni fenêtre.
struct Vie {
    hasard: Hasard,
    fils: Vec<Fil>,
    /// Les étincelles : angle de départ, rayon, vitesse, profondeur ; et leurs couleurs, en lumière.
    etincelles: Vec<[f64; 4]>,
    couleurs_etincelles: Vec<f32>,
    /// La lumière d'un point selon son âge sur le fil : la tête plus que la queue.
    age: [f64; POINTS],
    etat: &'static str,
    cible: Apparence,
    cur: Apparence,
    niveau_micro: f64,
    /// Le temps des orbites (il avance à la vitesse de l'état), la pensée et son prochain passage, la prochaine
    /// impulsion (en réflexion), le temps de la poussière et des étincelles, l'horloge (l'instant d'egui).
    t: f64,
    pensee: f64,
    prochaine_pensee: f64,
    prochain_pouls: f64,
    t_poussiere: f64,
    t_etincelles: f64,
    horloge: f64,
    /// Rien n'est encore affiché : le premier état se pose d'emblée, sans transition.
    deja_vu: bool,
    /// L'allègement (la transcription) et la réduction (en petit), qui glissent de 0 à 1.
    allege: f64,
    reduit: f64,
    petite: bool,
    vue: Vue,
    /// Les impulsions : une au plus par fil, sa position le long du fil (de la queue à la tête) ; 9 = aucune.
    pouls: [f64; N],
    vivant: Vivant,
    donnees: Vec<f32>,
}

impl Vie {
    fn nouvelle(graine: u64) -> Vie {
        let mut hasard = Hasard(graine);
        let fils = (0..N).map(|i| Fil::tirer(i, &mut hasard)).collect();
        let mut etincelles = Vec::with_capacity(ETINCELLES);
        let mut couleurs_etincelles = Vec::with_capacity(ETINCELLES * 3);
        for _ in 0..ETINCELLES {
            let angle = hasard.suivant() * TOUR;
            let rayon = 1.04 + hasard.suivant() * 0.2;
            let vitesse = (0.02 + hasard.suivant() * 0.06) * hasard.signe();
            etincelles.push([angle, rayon, vitesse, hasard.suivant()]);
            let couleur = PALETTE[(hasard.suivant() * PALETTE.len() as f64) as usize % PALETTE.len()];
            couleurs_etincelles.extend(lineaire(couleur).map(|x| x as f32));
        }
        let (etat, cible) = ETATS[0];
        Vie {
            hasard,
            fils,
            etincelles,
            couleurs_etincelles,
            age: std::array::from_fn(|q| (q as f64 / (POINTS - 1) as f64).powf(2.2)),
            etat,
            cible,
            cur: cible,
            niveau_micro: 0.0,
            t: 0.0,
            pensee: 0.0,
            prochaine_pensee: 4.0,
            prochain_pouls: 0.0,
            t_poussiere: 0.0,
            t_etincelles: 0.0,
            horloge: 0.0,
            deja_vu: false,
            allege: 0.0,
            reduit: 0.0,
            petite: false,
            vue: Vue { eclat: cible.eclat, vitesse: cible.vitesse, vie: 0.0 },
            pouls: [9.0; N],
            vivant: Vivant {
                hx: 0.0,
                hy: 0.0,
                dedans: false,
                survol: 0.0,
                onde: 0.0,
                onde_x: 0.0,
                onde_y: 0.0,
                elan: 0.0,
                rx: 0.0,
                ry: 0.0,
                cible_rx: 0.0,
                cible_ry: 0.0,
                vu: -10.0,
                prochain_regard: 3.0,
                oisif: 0.0,
                veille: 0.0,
            },
            donnees: vec![0.0; LARGEUR * N * 4],
        }
    }

    /// L'état publié (`poser` de la page) : un changement d'état réveille Eve ; le niveau du micro ne compte qu'en
    /// écoute ; le tout premier état se pose sans transition.
    fn poser(&mut self, nom: &str, niveau: f64, sec: f64) {
        let (etat, cible) = etat_connu(nom);
        if etat != self.etat {
            self.reveiller();
        }
        self.etat = etat;
        self.cible = cible;
        if etat == "ecoute" {
            self.niveau_micro = niveau;
        }
        if !self.deja_vu {
            self.cur = cible;
            self.allege = if etat == "transcription" { 1.0 } else { 0.0 };
            if etat == "confirmation" {
                (self.vivant.cible_rx, self.vivant.rx, self.vivant.cible_ry, self.vivant.ry) = (0.0, 0.0, 0.55, 0.55);
            }
            self.avancer(0.0, 0.0, sec);
        }
    }

    fn reveiller(&mut self) {
        if self.vivant.veille > 0.3 {
            (self.vivant.onde, self.vivant.onde_x, self.vivant.onde_y) = (1.0, 0.0, 0.0);
        }
        self.vivant.oisif = 0.0;
    }

    fn lancer_pouls(&mut self, n: usize) {
        for _ in 0..n {
            for _ in 0..6 {
                let i = (self.hasard.suivant() * N as f64) as usize % N;
                if self.pouls[i] > 5.0 {
                    self.pouls[i] = -0.15;
                    break;
                }
            }
        }
    }

    /// La souris a bougé (en rayons de sphère) : elle arrive sur Eve, une onde part de son cœur, vers elle.
    fn pointeur(&mut self, x: f64, y: f64, sec: f64) {
        let v = &mut self.vivant;
        (v.hx, v.hy, v.vu) = (x, y, sec);
        let dedans = x.hypot(y) < 1.05;
        if dedans && !v.dedans && v.survol < 0.3 {
            (v.onde, v.onde_x, v.onde_y) = (1.0, x * 0.25, y * 0.25);
        }
        v.dedans = dedans;
        self.reveiller();
    }

    /// Toucher Eve : un élan de lumière, une onde au point touché, trois impulsions.
    fn toucher(&mut self, x: f64, y: f64) {
        let v = &mut self.vivant;
        (v.elan, v.onde, v.onde_x, v.onde_y) = (1.0, 1.0, x * 0.3, y * 0.3);
        self.lancer_pouls(3);
        self.reveiller();
    }

    /// Le temps qui passe (`avancer` de la page) : l'état glisse vers sa cible, la vie suit son cours. `dt_vie` est le
    /// temps vrai (les transitions, les minuteries, le sommeil) ; `dt`, le pas des mouvements, plafonné (`PAS_MAX`).
    fn avancer(&mut self, dt_vie: f64, dt: f64, sec: f64) {
        self.horloge = sec;
        let k = 1.0 - (-dt_vie * 2.6).exp();
        let (c, b) = (&mut self.cur, &self.cible);
        c.vitesse = glisser(c.vitesse, b.vitesse, k);
        c.agitation = glisser(c.agitation, b.agitation, k);
        c.eclat = glisser(c.eclat, b.eclat, k);
        c.melange = glisser(c.melange, b.melange, k);
        for i in 0..3 {
            c.coeur[i] = glisser(c.coeur[i], b.coeur[i], k);
            c.teinte[i] = glisser(c.teinte[i], b.teinte[i], k);
        }
        let transcrit = self.etat == "transcription";
        self.allege = glisser(self.allege, if transcrit { 1.0 } else { 0.0 }, 1.0 - (-dt_vie * 8.0).exp());
        self.reduit = glisser(self.reduit, if self.petite { 1.0 } else { 0.0 }, 1.0 - (-dt_vie * 5.0).exp());
        // Une « pensée » au repos : l'orbe s'anime un instant, une impulsion court le long d'un fil.
        if self.etat == "pret" {
            self.prochaine_pensee -= dt_vie;
            if self.prochaine_pensee <= 0.0 {
                self.pensee = 1.0;
                self.prochaine_pensee = 5.0 + self.hasard.suivant() * 7.0;
                let n = if self.hasard.suivant() < 0.5 { 1 } else { 2 };
                self.lancer_pouls(n);
            }
        }
        self.pensee = (self.pensee - dt_vie * 0.7).max(0.0);
        // Quand elle réfléchit (ou transcrit), les impulsions se suivent : on la voit travailler.
        if self.etat == "reflexion" || transcrit {
            self.prochain_pouls -= dt_vie;
            if self.prochain_pouls <= 0.0 {
                self.lancer_pouls(1);
                self.prochain_pouls = 0.16 + self.hasard.suivant() * 0.34;
            }
        }
        let vie = match self.etat {
            "ecoute" => self.niveau_micro,
            "parole" => 0.25 + 0.45 * souffle(self.t * 9.1).abs() * (0.6 + 0.4 * souffle(self.t * 2.3)),
            _ => self.pensee * 0.6,
        };
        // Le sommeil : au repos depuis quatre minutes, Eve somnole ; la souris, une touche ou une parole la réveillent.
        let v = &mut self.vivant;
        v.oisif += dt_vie;
        let somnole = self.etat == "pret" && v.oisif > SOMMEIL;
        let vite = if somnole { 0.35 } else { 3.0 };
        v.veille = glisser(v.veille, if somnole { 1.0 } else { 0.0 }, 1.0 - (-dt_vie * vite).exp());
        let vite = if v.dedans { 6.0 } else { 1.8 };
        v.survol = glisser(v.survol, if v.dedans { 1.0 } else { 0.0 }, 1.0 - (-dt_vie * vite).exp());
        v.onde = (v.onde - dt_vie * 1.2).max(0.0);
        v.elan = (v.elan - dt_vie * 0.9).max(0.0);
        // Le regard : vers la carte quand Eve attend un accord ; vers la souris quand elle bouge (plus fort de près) ;
        // sinon, de temps en temps, un coup d'œil ailleurs.
        if self.etat == "confirmation" {
            (v.cible_rx, v.cible_ry) = (0.0, 0.55);
        } else if sec - v.vu < 2.5 {
            let d = v.hx.hypot(v.hy);
            let d = if d > 0.0 { d } else { 1.0 };
            let f = d.min(1.0) * (1.8 / d).min(1.0);
            (v.cible_rx, v.cible_ry) = (v.hx / d * f, v.hy / d * f);
        } else {
            v.prochain_regard -= dt_vie;
            if v.prochain_regard <= 0.0 {
                v.prochain_regard = 2.0 + self.hasard.suivant() * 4.0;
                let a = self.hasard.suivant() * TOUR;
                let f = self.hasard.suivant() * 0.45;
                (v.cible_rx, v.cible_ry) = (a.cos() * f, a.sin() * f);
            }
        }
        // Le regard glisse au pas des mouvements : un coup d'œil est une dérive douce, pas un bond de l'écheveau.
        let kr = 1.0 - (-dt * 3.5).exp();
        v.rx = glisser(v.rx, v.cible_rx, kr);
        v.ry = glisser(v.ry, v.cible_ry, kr);
        let (fs, veille, elan) = (v.survol, v.veille, v.elan);
        self.vue.vie = vie;
        self.vue.eclat = (self.cur.eclat * (1.0 - 0.55 * veille) + 0.22 * fs + 0.25 * elan).min(1.2);
        self.vue.vitesse =
            self.cur.vitesse * (1.0 + self.pensee * 1.4) * (1.0 + 0.6 * fs + 1.2 * elan) * (1.0 - 0.65 * veille);
        let v = self.vue.vitesse;
        self.t += dt * v;
        self.t_poussiere += dt * (0.3 + 0.7 * v);
        self.t_etincelles += dt * (1.0 + v);
        for p in &mut self.pouls {
            if *p < 5.0 {
                *p += dt * (0.9 + 0.3 * v);
                if *p > 1.3 {
                    *p = 9.0;
                }
            }
        }
        // Les fils avancent sur leur orbite, et leur orbite précesse.
        if dt > 0.0 {
            for f in &mut self.fils {
                f.phase += dt * v * f.w * 1.7;
                let (ay, ax) = (dt * v * f.prec[0], dt * v * f.prec[1]);
                let a = unitaire(tourner(f.ax, ay, ax));
                let b = tourner(f.e1, ay, ax);
                let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let b = unitaire([b[0] - a[0] * d, b[1] - a[1] * d, b[2] - a[2] * d]);
                (f.ax, f.e1, f.e2) = (a, b, croix(a, b));
            }
        }
    }

    /// Les courbes de l'image (`preparerFils` de la page) : 44 × 33 points, posés dans la texture des données. Le point
    /// est en rayons de sphère (y vers le bas), vu de l'intérieur du verre ; le verre le réfracte au dessin. `l` : la
    /// taille de la surface de la lumière, en pixels (un fil n'y est jamais plus fin que 0,62 pixel).
    fn preparer(&mut self, l: f64) {
        let (ag, m, tn) = (self.cur.agitation, self.cur.melange, lineaire(self.cur.teinte));
        let (eclat, vie) = (self.vue.eclat, self.vue.vie);
        let v = &self.vivant;
        let (elan, fs) = (v.elan, v.survol);
        let larg_min = 0.62 / (l / 2.0 * SPHERE).max(1.0);
        // Le regard : les fils se tournent un peu vers ce qu'elle regarde, et s'y portent (le cœur, davantage).
        let (a1, a2) = (v.rx * 0.38, v.ry * 0.38);
        let (c1, s1, c2, s2) = (a1.cos(), a1.sin(), a2.cos(), a2.sin());
        let (ox, oy, kx, ky) = (v.rx * 0.08, v.ry * 0.08, v.rx * 0.17, v.ry * 0.17);
        // La souris, vue à travers le verre ; l'onde qui passe pousse les fils.
        let h2 = (v.hx * v.hx + v.hy * v.hy).min(1.0);
        let hl = 1.0 - LENTILLE * h2 * h2;
        let (hx, hy) = (v.hx * hl, v.hy * hl);
        let (onde, onde_x, onde_y) = (v.onde, v.onde_x, v.onde_y);
        let ro = 0.12 + 0.9 * (1.0 - onde);
        let amp = 0.045 * (0.5 + ag);
        let amp2 = 0.05 * (0.4 + ag);
        let d = &mut self.donnees;
        for (i, f) in self.fils.iter().enumerate() {
            let ligne = i * LARGEUR * 4;
            // Allégée, l'orbe ne garde qu'un fil sur deux (en petit, deux sur trois) ; les autres s'effacent, puis ne
            // sont plus calculés (des boîtes vides, que le shader saute).
            let garde =
                (if i % 2 == 0 { 1.0 } else { 1.0 - self.allege }) * (if i % 3 == 2 { 1.0 - self.reduit } else { 1.0 });
            if garde < 0.02 {
                for b in (ligne + C_BOITES * 4..ligne + LARGEUR * 4).step_by(4) {
                    d[b..b + 4].copy_from_slice(&[9.0, 9.0, -9.0, -9.0]);
                }
                d[ligne + C_TETE * 4 + 2] = 0.0;
                continue;
            }
            let rr = f.r * (1.0 + 0.12 * ag * souffle(self.t * 1.9 + f.o) + 0.26 * vie + 0.12 * elan);
            // L'arc, de la queue à la tête. Les sinus avancent par rotation, d'un point au suivant.
            let pas = f.arc / (POINTS - 1) as f64;
            let th0 = f.phase - f.arc;
            let (mut ct, mut st) = (th0.cos(), th0.sin());
            let (cd, sd) = (pas.cos(), pas.sin());
            // L'orbite ondule doucement (des lobes qui respirent) et sort un peu de son plan.
            let a = f.lobes * th0 + self.t * 1.3 + f.o;
            let da = f.lobes * pas;
            let bb = f.lobes2 * th0 + self.t * 0.9 + f.o2;
            let db = f.lobes2 * pas;
            let (mut s1a, mut c1a) = (a.sin(), a.cos());
            let (mut s2a, mut c2a) = ((2.17 * a + 1.3).sin(), (2.17 * a + 1.3).cos());
            let (mut sb, mut cb) = (bb.sin(), bb.cos());
            let (c1d, s1d, c2d, s2d) = (da.cos(), da.sin(), (2.17 * da).cos(), (2.17 * da).sin());
            let (cbd, sbd) = (db.cos(), db.sin());
            let pp = self.pouls[i];
            let (mut x0, mut y0, mut x1, mut y1) = (1e9_f64, 1e9_f64, -1e9_f64, -1e9_f64);
            let (mut x, mut y, mut intensite, mut lw) = (0.0, 0.0, 0.0, LARGE);
            for q in 0..POINTS {
                let r3 = rr * (1.0 + amp * (0.7 * s1a + 0.3 * s2a));
                let h3 = rr * amp2 * sb;
                let mut px = r3 * (f.e1[0] * ct + f.e2[0] * st) + f.ax[0] * h3;
                let mut py = r3 * (f.e1[1] * ct + f.e2[1] * st) + f.ax[1] * h3;
                let mut pz = r3 * (f.e1[2] * ct + f.e2[2] * st) + f.ax[2] * h3;
                // Les points restent dans la sphère (rayon 0,97).
                let n2 = px * px + py * py + pz * pz;
                if n2 > 0.9409 {
                    let k = 0.97 / n2.sqrt();
                    (px, py, pz) = (px * k, py * k, pz * k);
                }
                let x1r = px * c1 + pz * s1;
                let z1 = pz * c1 - px * s1;
                let y2 = py * c2 + z1 * s2;
                let z2 = z1 * c2 - py * s2;
                let sp = ECHELLE / (1.0 - PERSPECTIVE * z2);
                x = ox + x1r * sp;
                y = oy + y2 * sp;
                // Sous la souris, les fils s'en approchent un peu, comme attirés.
                if fs > 0.01 {
                    let (dx, dy) = (hx - x, hy - y);
                    let tire = 0.2 * fs * (-(dx * dx + dy * dy) * 2.4).exp();
                    x += dx * tire;
                    y += dy * tire;
                }
                if onde > 0.01 {
                    let (dx, dy) = (x - onde_x, y - onde_y);
                    let dist = (dx * dx + dy * dy).sqrt() + 1e-6;
                    let b = (dist - ro) / 0.08;
                    let pousse = 0.035 * onde * (-b * b).exp() / dist;
                    x += dx * pousse;
                    y += dy * pousse;
                }
                // La lumière du point : la tête plus que la queue, l'avant plus que le fond, et plus vive près du cœur.
                // Le fond est aussi plus flou (plus large) : on le voit à travers plus de verre.
                let avant = (z2 + 1.0) * 0.5;
                let (dx, dy) = (x - kx, y - ky);
                intensite = eclat
                    * f.gain
                    * garde
                    * self.age[q]
                    * (0.08 + 0.92 * avant * avant)
                    * (1.0 + 0.5 / (1.0 + (dx * dx + dy * dy) / 0.07));
                if pp < 5.0 {
                    let b = (q as f64 / (POINTS - 1) as f64 - pp) / 0.055;
                    intensite *= 1.0 + 2.6 * (-b * b).exp();
                }
                lw = (LARGE * (1.0 + 1.5 * (1.0 - avant) * (1.0 - avant))).max(larg_min);
                let k = ligne + q * 4;
                d[k..k + 4].copy_from_slice(&[
                    x as f32,
                    y as f32,
                    (intensite * INTENSITE) as f32,
                    (1.0 / (lw * lw)) as f32,
                ]);
                // La boîte de chaque morceau, élargie de la portée de la lueur.
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                if q > 0 && q % SEG == 0 {
                    let b = ligne + (C_BOITES + q / SEG - 1) * 4;
                    let boite = [x0 - PORTEE, y0 - PORTEE, x1 + PORTEE, y1 + PORTEE];
                    d[b..b + 4].copy_from_slice(&boite.map(|v| v as f32));
                    (x0, x1, y0, y1) = (x, x, y, y);
                }
                (ct, st) = (ct * cd - st * sd, st * cd + ct * sd);
                (c1a, s1a) = (c1a * c1d - s1a * s1d, s1a * c1d + c1a * s1d);
                (c2a, s2a) = (c2a * c2d - s2a * s2d, s2a * c2d + c2a * s2d);
                (cb, sb) = (cb * cbd - sb * sbd, sb * cbd + cb * sbd);
            }
            // La tête : un point plus vif, plus large ; il éclate quand une impulsion y arrive.
            let eclair = if pp < 5.0 { 1.0 + 3.0 * (-((1.0 - pp) / 0.08).powi(2)).exp() } else { 1.0 };
            let lt = lw * 2.4;
            let kt = ligne + C_TETE * 4;
            d[kt..kt + 4].copy_from_slice(&[
                x as f32,
                y as f32,
                (intensite * INTENSITE * eclair) as f32,
                (1.0 / (lt * lt)) as f32,
            ]);
            let kc = ligne + C_COULEUR * 4;
            for j in 0..3 {
                d[kc + j] = (f.c[j] * (1.0 - m) + tn[j] * m) as f32;
            }
            d[kc + 3] = 1.0;
        }
    }

    /// Les uniformes de l'image (le cœur, l'onde, la poussière, la flamme, le verre, les étincelles), pour une surface
    /// de lumière de `l` pixels, dans le thème clair ou sombre.
    fn uniformes(&self, l: f64, clair: bool) -> Uniformes {
        let r = l / 2.0 * SPHERE;
        let v = &self.vivant;
        let (vie, fs, elan, veille, eclat) = (self.vue.vie, v.survol, v.elan, v.veille, self.vue.eclat);
        let rc = (0.13 + 0.04 * souffle(self.t * 1.25) + 0.2 * vie + 0.05 * self.pensee + 0.05 * fs + 0.08 * elan)
            * (1.0 - 0.35 * veille);
        let (kx, ky) = (v.rx * 0.17, v.ry * 0.17);
        let f = |x: f64| x as f32;
        // L'angle du moment de chaque étincelle se calcule ici, en double précision, et part déjà réduit : un temps
        // qui grandit des jours durant y perdrait sa précision.
        let mut et = Vec::with_capacity(ETINCELLES * 4);
        for e in &self.etincelles {
            et.extend([f((e[0] + e[2] * self.t_etincelles) % TOUR), f(e[1]), f(e[0]), f(e[3])]);
        }
        // Le studio que le verre renvoie : plus clair quand la page l'est ; l'intérieur reste sombre dans les deux.
        let (ciel, sol, fenetre) = if clair {
            ([0.26, 0.29, 0.33], [0.05, 0.055, 0.065], 9.0)
        } else {
            ([0.05, 0.06, 0.085], [0.006, 0.007, 0.01], 6.0)
        };
        Uniformes {
            coeur: [f(kx), f(ky), f(rc), f(eclat * ENERGIE)],
            coeur_c: lineaire(self.cur.coeur).map(f),
            onde: [f(v.onde_x), f(v.onde_y), f(0.12 + 0.9 * (1.0 - v.onde)), f(v.onde)],
            pous: [
                f((self.t_poussiere * 0.05) % TOUR),
                f((-self.t_poussiere * 0.035) % TOUR),
                f(eclat),
                f((0.6 / r).max(0.0042)),
            ],
            flamme: [
                f((self.t_poussiere * 1.9) % TOUR),
                f((self.t_poussiere * 1.3) % TOUR),
                f(self.horloge % 3600.0),
                0.0,
            ],
            teinte: lineaire(self.cur.teinte).map(f),
            vie: [
                f((0.18 + 0.2 * vie + 0.2 * fs + 0.25 * elan) * (1.0 - 0.5 * veille) * 0.5),
                f((0.55 + 0.3 * vie + 0.2 * fs) * 0.9),
                f(eclat),
                f(fs),
            ],
            etin: [f(elan), f(eclat), f(self.horloge % 3600.0), 0.0],
            et,
            couleurs_et: self.couleurs_etincelles.clone(),
            bloom: f(0.035 * (1.0 - self.allege)),
            eclat_flou: self.allege < 0.98,
            clair: if clair { 1.0 } else { 0.0 },
            ciel,
            sol,
            fenetre,
        }
    }

    /// L'orbe s'anime : un état actif, une transition qui glisse encore, une pensée, une onde, un élan, une impulsion
    /// en route, la souris sur elle ou qui bouge, le sommeil qui vient ou s'en va. Au repos et posée : non.
    fn s_anime(&self, sec: f64) -> bool {
        let actif = !matches!(self.etat, "pret" | "demarrage" | "erreur");
        let (c, b) = (&self.cur, &self.cible);
        let ecart = |x: f64, y: f64, seuil: f64| (x - y).abs() > seuil;
        let glisse = ecart(c.vitesse, b.vitesse, 0.004)
            || ecart(c.agitation, b.agitation, 0.004)
            || ecart(c.eclat, b.eclat, 0.004)
            || ecart(c.melange, b.melange, 0.004)
            || (0..3).any(|i| ecart(c.coeur[i], b.coeur[i], 0.8) || ecart(c.teinte[i], b.teinte[i], 0.8));
        let v = &self.vivant;
        let allege = if self.etat == "transcription" { 1.0 } else { 0.0 };
        let reduit = if self.petite { 1.0 } else { 0.0 };
        let veille = if self.etat == "pret" && v.oisif > SOMMEIL { 1.0 } else { 0.0 };
        actif
            || glisse
            || self.pensee > 0.0
            || v.onde > 0.0
            || v.elan > 0.0
            || self.pouls.iter().any(|p| *p < 5.0)
            || v.survol > 0.01
            || sec - v.vu < 2.5
            || ecart(self.allege, allege, 0.01)
            || ecart(self.reduit, reduit, 0.01)
            || ecart(v.veille, veille, 0.01)
    }
}

/// Les uniformes d'une image, prêts pour les shaders (en f32).
struct Uniformes {
    coeur: [f32; 4],
    coeur_c: [f32; 3],
    onde: [f32; 4],
    pous: [f32; 4],
    flamme: [f32; 4],
    teinte: [f32; 3],
    vie: [f32; 4],
    etin: [f32; 4],
    et: Vec<f32>,
    couleurs_et: Vec<f32>,
    bloom: f32,
    eclat_flou: bool,
    clair: f32,
    ciel: [f32; 3],
    sol: [f32; 3],
    fenetre: f32,
}

/// Ce que le rappel reçoit pour une image : les courbes et les uniformes, tout prêts ; la taille de la lumière (ses
/// surfaces) et celle du carré de l'orbe à l'écran, en pixels.
struct Image {
    donnees: Vec<f32>,
    u: Uniformes,
    l_lumiere: i32,
    l_ecran: i32,
}

/// L'orbe : ce qui vit d'une image à l'autre ([`Vie`]), et les ressources d'OpenGL, partagées avec le rappel qui
/// dessine (sur le fil du rendu).
pub struct Orbe {
    vie: Vie,
    partage: Arc<Mutex<Partage>>,
    /// L'instant de l'image précédente : le temps qui passe d'une image à l'autre.
    precedent: Option<f64>,
    /// Le temps de la dernière préparation (le processeur), pour `eve --essai-orbe --mesure`.
    preparation: Duration,
}

/// Ce que le rappel et l'orbe se partagent.
struct Partage {
    rendu: Rendu,
    /// 0,6 sur un rendu logiciel (la lumière moins fine : le processeur doit rester à l'oreille), 1 sinon.
    qualite: f64,
    /// Mesurer les trois passes (glFinish avant et après : pour un essai seulement).
    mesurer: bool,
    /// Les dernières durées des trois passes, en millisecondes.
    durees: Vec<f64>,
}

enum Rendu {
    /// Rien n'est encore prêt : le premier rappel prépare OpenGL.
    AFaire,
    Pret(Box<Gl>),
    /// OpenGL a refusé quelque chose : le disque peint par egui remplace l'orbe.
    EnPanne,
    /// Les ressources sont rendues (la fenêtre se ferme).
    Libere,
}

impl Orbe {
    /// Une orbe neuve ; son écheveau change à chaque lancement, comme dans la page.
    pub fn nouvelle() -> Orbe {
        let horloge = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x5EED);
        let graine = horloge ^ u64::from(std::process::id()).rotate_left(32);
        Orbe {
            vie: Vie::nouvelle(graine),
            partage: Arc::new(Mutex::new(Partage {
                rendu: Rendu::AFaire,
                qualite: 1.0,
                mesurer: false,
                durees: Vec::new(),
            })),
            precedent: None,
            preparation: Duration::ZERO,
        }
    }

    /// Dessine l'orbe dans `rect` (le plus grand carré qui y tient, centré), pour l'état d'Eve (« pret », « ecoute »,
    /// « transcription », « reflexion », « parole », « confirmation », « demarrage », « erreur » — les noms de l'état
    /// publié ; un autre vaut « pret »), le niveau du micro (0 à 1, lu en écoute) et l'instant (en secondes : celui
    /// d'egui, `ui.input(|i| i.time)`). Rend vrai tant qu'elle s'anime (un état actif, une transition, une pensée, une
    /// onde, la souris sur elle), faux au repos.
    ///
    /// Elle demande elle-même l'image suivante : 30 par seconde quand elle s'anime, 15 en transcription (allégée, pour
    /// laisser le processeur à Whisper), 4 au repos — la fenêtre n'a pas à en demander davantage pour elle. Elle lit la
    /// souris et le clavier (le regard, l'onde, le réveil, l'élan d'un clic) sans prendre les clics : la fenêtre garde
    /// le geste « tenir pour parler » sur le même rectangle.
    pub fn dessiner(&mut self, ui: &mut egui::Ui, rect: Rect, etat: &str, niveau: f32, temps: f64) -> bool {
        let cote = rect.width().min(rect.height());
        if cote.is_nan() || cote < 2.0 || !temps.is_finite() {
            return false;
        }
        let niveau = if niveau.is_finite() { f64::from(niveau.clamp(0.0, 1.0)) } else { 0.0 };
        let debut = Instant::now();
        let carre = Rect::from_center_size(rect.center(), egui::vec2(cote, cote));
        let dt_vie = self.precedent.map_or(0.0, |p| (temps - p).clamp(0.0, 0.5));
        self.precedent = Some(temps);
        self.vie.petite = cote < PETITE;
        if !self.vie.deja_vu {
            self.vie.reduit = if self.vie.petite { 1.0 } else { 0.0 };
        }
        self.vie.poser(etat, niveau, temps);
        self.ecouter(ui, carre, temps);
        self.vie.avancer(dt_vie, dt_vie.min(PAS_MAX), temps);
        let (qualite, opengl) = match self.partage.lock() {
            Ok(p) => (p.qualite, matches!(p.rendu, Rendu::AFaire | Rendu::Pret(_))),
            Err(_) => (1.0, false),
        };
        if !opengl {
            repli(ui.painter(), carre, self.vie.cible.teinte);
            self.vie.deja_vu = true;
            return false;
        }
        if ui.is_rect_visible(carre) {
            let pixels = f64::from(cote * ui.ctx().pixels_per_point());
            let l_ecran = pixels.round().max(2.0) as i32;
            let l_lumiere = (pixels * qualite).round().max(2.0) as i32;
            self.vie.preparer(f64::from(l_lumiere));
            let image = Image {
                donnees: self.vie.donnees.clone(),
                u: self.vie.uniformes(f64::from(l_lumiere), !ui.visuals().dark_mode),
                l_lumiere,
                l_ecran,
            };
            let partage = Arc::clone(&self.partage);
            let dessin = egui_glow::CallbackFn::new(move |info, painter| rappel(&partage, &image, &info, painter));
            ui.painter().add(egui::PaintCallback { rect: carre, callback: Arc::new(dessin) });
        }
        self.vie.deja_vu = true;
        self.preparation = debut.elapsed();
        let anime = self.vie.s_anime(temps);
        let cadence = match (anime, self.vie.etat) {
            (false, _) => IMAGES_AU_REPOS,
            (true, "transcription") => IMAGES_ALLEGEES,
            (true, _) => IMAGES,
        };
        ui.ctx().request_repaint_after(Duration::from_secs_f64(1.0 / cadence));
        anime
    }

    /// Rend les ressources d'OpenGL (de `App::on_exit`, qui reçoit le contexte avant qu'il ne s'éteigne).
    pub fn liberer(&mut self, gl: &glow::Context) {
        if let Ok(mut p) = self.partage.lock()
            && let Rendu::Pret(g) = std::mem::replace(&mut p.rendu, Rendu::Libere)
        {
            // SAFETY : `on_exit` est appelé sur le fil du rendu, le contexte d'egui encore courant.
            unsafe { g.detruire(gl) };
        }
    }

    /// Mesure désormais chaque image (`eve --essai-orbe --mesure`) : la carte graphique est attendue avant et après
    /// les trois passes, ce qui ralentit un peu tout le reste.
    pub fn mesurer(&mut self) {
        if let Ok(mut p) = self.partage.lock() {
            p.mesurer = true;
        }
    }

    /// Ce que coûte une image, en millisecondes : sa préparation sur le processeur (la dernière), et ses trois passes
    /// sur la carte graphique (la médiane des dernières ; rien sans [`Orbe::mesurer`]).
    pub fn mesures(&self) -> (f64, Option<f64>) {
        let gpu = self.partage.lock().ok().and_then(|p| {
            let mut d = p.durees.clone();
            d.sort_by(f64::total_cmp);
            d.get(d.len() / 2).copied()
        });
        (self.preparation.as_secs_f64() * 1000.0, gpu)
    }

    /// Le disque d'emblée, comme sans OpenGL (`eve --essai-orbe --repli`).
    pub fn sans_opengl(&mut self) {
        if let Ok(mut p) = self.partage.lock() {
            p.rendu = Rendu::EnPanne;
        }
    }

    /// La souris et le clavier de l'image : le regard, l'onde, le réveil, l'élan d'un clic sur l'orbe. Rien n'est
    /// pris : les clics restent à la fenêtre.
    fn ecouter(&mut self, ui: &egui::Ui, carre: Rect, temps: f64) {
        let centre = carre.center();
        let rayon = f64::from(carre.width() / 2.0) * SPHERE;
        let vers = |p: Pos2| (f64::from(p.x - centre.x) / rayon, f64::from(p.y - centre.y) / rayon);
        let vie = &mut self.vie;
        ui.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::PointerMoved(p) => {
                        let (x, y) = vers(*p);
                        vie.pointeur(x, y, temps);
                    }
                    egui::Event::PointerGone => vie.vivant.dedans = false,
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                        if carre.contains(*pos) =>
                    {
                        let (x, y) = vers(*pos);
                        vie.toucher(x, y);
                    }
                    egui::Event::Key { pressed: true, .. } => vie.reveiller(),
                    _ => {}
                }
            }
        });
    }
}

/// Sans OpenGL : un disque sombre et son liseré, deux anneaux autour, peints par egui (le repli de la page :
/// `radial-gradient` de #17212d à #05080c, immobile).
fn repli(p: &egui::Painter, carre: Rect, teinte: [f64; 3]) {
    let c = carre.center();
    let r = carre.width() / 2.0 * SPHERE as f32;
    let arrets = [
        (0.0, Color32::from_rgb(0x17, 0x21, 0x2D)),
        (0.674, Color32::from_rgb(0x0B, 0x11, 0x19)),
        (1.0, Color32::from_rgb(0x05, 0x08, 0x0C)),
    ];
    disque(p, c, r, &arrets);
    // Le liseré prend la teinte de l'état, passée par le même tone mapping que le verre.
    let [rouge, vert, bleu] = lineaire(teinte).map(|x| (vers_srgb(aces(x * 1.6)) * 255.0).round() as u8);
    let lisere = Color32::from_rgba_unmultiplied(rouge, vert, bleu, 150);
    p.circle_stroke(c, r, (1.4, lisere));
    p.circle_stroke(c, r * 1.07, (1.0, lisere.gamma_multiply(0.3)));
    p.circle_stroke(c, r * 1.13, (1.0, lisere.gamma_multiply(0.14)));
}

/// Un disque en dégradé radial : ses arrêts, du centre (0) au bord (1).
fn disque(p: &egui::Painter, c: Pos2, r: f32, arrets: &[(f32, Color32)]) {
    const PAS: u32 = 72;
    if arrets.len() < 2 {
        return;
    }
    let mut m = egui::Mesh::default();
    m.colored_vertex(c, arrets[0].1);
    for &(f, couleur) in &arrets[1..] {
        for k in 0..PAS {
            let a = k as f32 / PAS as f32 * std::f32::consts::TAU;
            m.colored_vertex(c + egui::vec2(a.cos(), a.sin()) * (r * f), couleur);
        }
    }
    for k in 0..PAS {
        m.add_triangle(0, 1 + k, 1 + (k + 1) % PAS);
    }
    for anneau in 0..(arrets.len() as u32 - 2) {
        let (a, b) = (1 + anneau * PAS, 1 + (anneau + 1) * PAS);
        for k in 0..PAS {
            let k1 = (k + 1) % PAS;
            m.add_triangle(a + k, b + k, b + k1);
            m.add_triangle(a + k, b + k1, a + k1);
        }
    }
    p.add(egui::Shape::mesh(m));
}

// ---- Les shaders : ceux de la page, en GLSL de bureau (1.40) ou d'OpenGL ES (3.00) --------------------------------
// Leurs commentaires sont retirés avant la compilation (`sans_commentaires`) : certains pilotes refusent un caractère
// accentué, même dans un commentaire.

/// Le sommet commun : un triangle qui couvre tout le viewport, tiré de `gl_VertexID`, sans attribut.
const SOMMET: &str = r#"
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2. - 1., 0., 1.);
}
"#;

/// La lumière, vue de l'intérieur : les fils, le cœur, la poussière, l'onde. En lumière linéaire, sans limite.
const LUMIERE: &str = r#"
uniform sampler2D uFils;
uniform vec2 uCentre;
uniform float uR;
uniform vec4 uCoeur;   // position (x, y), taille, énergie
uniform vec3 uCoeurC;  // couleur du cœur
uniform vec4 uOnde;    // centre (x, y), rayon, force
uniform vec4 uPous;    // angles des deux couches de poussière, éclat, taille d'un grain
uniform vec4 uFlamme;  // deux phases de la flamme du cœur, horloge (en secondes, modulo une heure)
out vec4 sortie;
float hash12(vec2 p) { vec3 q = fract(p.xyx * .1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
vec2 hash22(vec2 p) {
  vec3 q = fract(p.xyx * vec3(.1031, .1030, .0973)); q += dot(q, q.yzx + 33.33); return fract((q.xx + q.yz) * q.zy);
}
// Une couche de poussière : au plus un grain par case, qui scintille.
float grains(vec2 p, float echelle, float angle, float graine, float taille, float horloge) {
  float c = cos(angle), s = sin(angle);
  vec2 q = vec2(c * p.x - s * p.y, s * p.x + c * p.y) * echelle + graine;
  vec2 id = floor(q);
  float h = hash12(id);
  if (h < .72) return 0.;
  vec2 d = (fract(q) - .2 - .6 * hash22(id + 7.31)) / echelle;
  float sc = max(0., .35 + .65 * sin(horloge * (.6 + 2.4 * fract(h * 37.1)) + h * 61.7));
  return (h - .72) / .28 * sc * exp(-dot(d, d) / (taille * taille));
}
void main() {
  vec2 p = vec2(gl_FragCoord.x - uCentre.x, uCentre.y - gl_FragCoord.y) / uR;
  if (dot(p, p) > .93) { sortie = vec4(0.); return; }
  float ip2 = 1. / (PORTEE * PORTEE);
  vec3 lum = vec3(0.);
  // Chaque fil : la lueur la plus forte de ses segments, plus sa tête. Le noyau : une lorentzienne L, surtout au
  // carré (net), un peu telle quelle (la lumière qui bave), ramenée à zéro à la portée.
  for (int i = 0; i < N_FILS; i++) {
    float v = 0.;
    for (int m = 0; m < MORCEAUX; m++) {
      vec4 b = texelFetch(uFils, ivec2(C_BOITES + m, i), 0);
      if (p.x < b.x || p.y < b.y || p.x > b.z || p.y > b.w) continue;
      vec4 a = texelFetch(uFils, ivec2(m * SEG, i), 0);
      for (int s = 1; s <= SEG; s++) {
        vec4 e = texelFetch(uFils, ivec2(m * SEG + s, i), 0);
        vec2 ab = e.xy - a.xy, ap = p - a.xy;
        float h = clamp(dot(ap, ab) / max(dot(ab, ab), 1e-12), 0., 1.);
        vec2 d = ap - ab * h;
        float d2 = dot(d, d), f = max(0., 1. - d2 * ip2), l = 1. / (1. + d2 * mix(a.w, e.w, h));
        v = max(v, mix(a.z, e.z, h) * l * (.25 + .75 * l) * f * f);
        a = e;
      }
    }
    vec4 t = texelFetch(uFils, ivec2(C_TETE, i), 0);
    vec2 dt = p - t.xy;
    float d2 = dot(dt, dt), f = max(0., 1. - d2 * ip2);
    v += t.z / (1. + d2 * t.w) * f * f;
    if (v > 0.) lum += texelFetch(uFils, ivec2(C_COULEUR, i), 0).rgb * v;
  }
  // Le cœur : un point blanc, un corps chaud (tous deux nets), et une lueur orangée qui s'éteint en 1/d² ; une
  // flamme lente le fait vivre.
  vec2 dc = p - uCoeur.xy;
  float d2c = dot(dc, dc), rc2 = uCoeur.z * uCoeur.z, dist = sqrt(d2c), ang = atan(dc.y, dc.x + 1e-6);
  float rl = min(uCoeur.z, .17);   // la lueur ne grandit pas avec la voix : le corps du cœur, oui
  float blanc = 1. / (1. + d2c / (.018 * rc2)), chaud = 1. / (1. + d2c / (.2 * rc2)),
        lueur = 1. / (1. + d2c / (.45 * rl * rl));
  float flamme = 1. + .16 * sin(ang * 3. + uFlamme.x - dist * 26.) * sin(ang * 5. - uFlamme.y + dist * 9.);
  lum += uCoeur.w * (vec3(1., .97, .9) * (blanc * blanc) + uCoeurC * (.9 * chaud * chaud * flamme)
                     + uCoeurC * uCoeurC * (.2 * lueur));
  // La poussière : deux couches qui dérivent en sens contraires (la profondeur), éclairées par le cœur en 1/d².
  float g = grains(p - uCoeur.xy * .35, 15., uPous.x, 0., uPous.w, uFlamme.z)
          + .55 * grains(p + uCoeur.xy * .15, 23., uPous.y, 17.3, uPous.w * .85, uFlamme.z * 1.37);
  lum += uCoeurC * (g * uPous.z * 2.4 / (1. + d2c / .08));
  // L'onde : un anneau qui part du cœur, et son sillage qui s'efface derrière lui.
  if (uOnde.w > 0.) {
    float d = length(p - uOnde.xy) - uOnde.z, x = d / (.012 * (1. + uOnde.w));
    lum += uCoeurC * (uOnde.w * (1.6 * exp(-x * x) + .16 * exp(min(d, 0.) / .07) * step(d, 0.)));
  }
#ifdef ENCODAGE
  lum = lum / (1. + lum);
#endif
  sortie = vec4(lum, 1.);
}
"#;

/// L'éclat, une réduction : la lumière de moitié en moitié (un flou large et doux, avec la remontée).
const DESCENTE: &str = r#"
uniform sampler2D uSource;
uniform vec2 uTexel, uDest;
out vec4 sortie;
void main() {
  vec2 uv = gl_FragCoord.xy / uDest, o = uTexel;
  vec3 s = texture(uSource, uv).rgb * 4. + texture(uSource, uv - o).rgb + texture(uSource, uv + o).rgb
         + texture(uSource, uv + vec2(o.x, -o.y)).rgb + texture(uSource, uv - vec2(o.x, -o.y)).rgb;
  sortie = vec4(s * .125, 1.);
}
"#;

/// L'éclat, une remontée : chaque échelle s'ajoute à la précédente.
const MONTEE: &str = r#"
uniform sampler2D uSource, uAjout;
uniform vec2 uTexel, uDest;
out vec4 sortie;
void main() {
  vec2 uv = gl_FragCoord.xy / uDest, o = uTexel;
  vec3 s = texture(uSource, uv + vec2(-o.x, 0.)).rgb + texture(uSource, uv + vec2(o.x, 0.)).rgb
         + texture(uSource, uv + vec2(0., -o.y)).rgb + texture(uSource, uv + vec2(0., o.y)).rgb
         + 2. * (texture(uSource, uv + .5 * o).rgb + texture(uSource, uv - .5 * o).rgb
         + texture(uSource, uv + .5 * vec2(o.x, -o.y)).rgb + texture(uSource, uv - .5 * vec2(o.x, -o.y)).rgb);
  sortie = vec4(s * (.6 / 12.) + texture(uAjout, uv).rgb, 1.);   // les grandes échelles comptent un peu moins
}
"#;

/// Le verre et ce qui l'entoure, puis le tone mapping, dans le framebuffer d'egui. La sortie est en alpha
/// prémultiplié : hors de la sphère, une lumière sans alpha s'ajoute au fond (le halo, les étincelles) et l'ombre
/// l'assombrit. Deux géométries : celle de l'écran (`uCentre`, `uR` : le carré de l'orbe dans la fenêtre) et celle des
/// surfaces de la lumière, qu'on y lit (`uCentreLu`, `uRLu`, `uTailleLu`).
const SCENE: &str = r#"
uniform sampler2D uLumiere, uEclat;
uniform vec2 uCentre, uCentreLu, uTailleLu;
uniform float uR, uRLu, uExpo, uClair, uBloom, uFenetre;
uniform vec3 uTeinte, uCoeurC, uCiel, uSol;
uniform vec4 uCoeur, uVie, uEtin;   // uVie : halo, liseré, éclat, survol ; uEtin : élan, éclat, horloge
uniform vec4 uEt[ETINCELLES];       // angle du moment, rayon, graine, profondeur
uniform vec3 uEtC[ETINCELLES];
out vec4 sortie;
vec3 aces(vec3 x) { return clamp(x * (2.51 * x + .03) / (x * (2.43 * x + .59) + .14), 0., 1.); }
vec3 versSrgb(vec3 c) { return mix(c * 12.92, 1.055 * pow(c, vec3(1. / 2.4)) - .055, step(.0031308, c)); }
vec2 uv(vec2 q) { return (vec2(q.x, -q.y) * uRLu + uCentreLu) / uTailleLu; }
vec3 lire(sampler2D t, vec2 a, vec2 b, vec2 c) {
  return vec3(texture(t, uv(a)).r, texture(t, uv(b)).g, texture(t, uv(c)).b);
}
// Le studio que le verre renvoie : un ciel plus clair que le sol, une grande fenêtre en haut à gauche (plus claire en
// haut), et une bande de contre-jour à droite, derrière, qui dessine un fin croissant sur le bord.
vec3 studio(vec3 d) {
  vec3 c = mix(uSol, uCiel, smoothstep(.25, -.45, d.y));
  vec3 s = normalize(vec3(-.55, -.7, .45)), su = normalize(vec3(s.y, -s.x, 0.)), sv = cross(su, s);
  float x = dot(d, su), y = dot(d, sv), f = smoothstep(.0, .35, dot(d, s));
  c += uFenetre * f * smoothstep(.40, .32, abs(x)) * smoothstep(.20, .14, abs(y))
       * (.08 + .92 * smoothstep(.2, -.2, y));
  return c + uFenetre * .045 * smoothstep(.8, .93, dot(d, normalize(vec3(.92, -.2, -.34))));
}
void main() {
  // gl_FragCoord compte depuis le coin de la fenêtre : uCentre est le centre du carré de l'orbe, décalé d'autant.
  vec2 p = vec2(gl_FragCoord.x - uCentre.x, uCentre.y - gl_FragCoord.y) / uR;
  float r2 = dot(p, p), r = sqrt(r2), px = 1. / uR;
  float couv = clamp((1. - r) / px + .5, 0., 1.);
  vec3 dedans = vec3(0.), dehors = vec3(0.);
  float ombre = 0.;
  if (couv > 0.) {
    float nz = sqrt(max(0., 1. - r2));
    vec3 n = vec3(p, nz);
    float fres = .04 + .96 * pow(1. - nz, 5.);
    // La réfraction : vers le bord, on voit l'intérieur plus près du centre, un peu moins pour le rouge que le bleu.
    float r4 = r2 * r2;
    vec2 pr = p * (1. - LENTILLE * .96 * r4), pg = p * (1. - LENTILLE * r4), pb = p * (1. - LENTILLE * 1.04 * r4);
    vec3 lum = lire(uLumiere, pr, pg, pb);
#ifdef ENCODAGE
    lum = lum / max(1. - lum, 1e-3);
#endif
    vec3 ecl = lire(uEclat, pr, pg, pb);
    // Le verre, sombre et bleuté, un peu plus clair en haut à gauche ; le cœur l'éclaire de l'intérieur.
    float fg = length(p - vec2(-.3, -.35));
    vec3 verre = mix(vec3(.022, .032, .052), vec3(.008, .011, .018), smoothstep(0., .95, fg));
    verre = mix(verre, vec3(.004, .0055, .008), smoothstep(.9, 1.35, fg));
    vec2 dc = pg - uCoeur.xy;
    verre += uCoeurC * (uVie.z * .025 / (1. + dot(dc, dc) / .07)) + uTeinte * (.006 + .02 * r2 * r2);
    vec3 interieur = verre + lum + ecl * uBloom;
    // Ce que le verre renvoie (Fresnel), et le point brillant de la lampe.
    vec3 refl = studio(vec3(2. * nz * p, 2. * nz * nz - 1.));
    float cs = max(dot(n, vec3(-.4598, -.4998, .7337)), 0.);
    float spec = pow(cs, 1800.) * 5. + pow(cs, 90.) * .12;
    // Le liseré : une ligne de la teinte, un peu dispersée (le rouge dehors, le bleu dedans), et une lueur intérieure.
    vec3 rr = vec3(r) + vec3(.0035, 0., -.0035), xl = (rr - .991) / .0065;
    vec3 lisere = uTeinte * (exp(-xl * xl) * uVie.y
                             + pow(smoothstep(vec3(.72), vec3(1.), rr), vec3(2.8)) * (.1 + .08 * uVie.w));
    // Le contre-reflet : la lumière du cœur renvoyée par le fond du verre, un mince croissant en bas à droite.
    float croissant = smoothstep(.86, .965, r) * smoothstep(.995, .965, r)
                      * pow(max(0., dot(p, vec2(.66, .75)) / max(r, 1e-3)), 6.);
    vec3 contre = uCoeurC * uVie.z * (.22 * croissant);
    dedans = (1. - fres) * interieur + fres * refl + vec3(spec) + lisere + contre;
  }
  if (couv < 1.) {
    // Le halo de la teinte, qui s'éteint avant le bord de la toile.
    dehors += mix(uTeinte, uCoeurC, .2) * (uVie.x * exp(-(r - 1.) / .055) * (1. - smoothstep(1.02, 1.16, r)));
    // Les étincelles, autour ; un élan les pousse vers l'extérieur.
    if (r > .97) {
      float bord = smoothstep(1.163, 1.12, max(abs(p.x), abs(p.y))), t2 = max(.0045, .75 * px);
      t2 *= t2;
      for (int i = 0; i < ETINCELLES; i++) {
        vec4 e = uEt[i];
        float a = e.x, re = e.y + .22 * uEtin.x;
        vec2 q = p - vec2(cos(a), .96 * sin(a)) * re;
        float d2 = dot(q, q);
        if (d2 > .0016) continue;
        float f = 1. - d2 / .0016, sc = .55 + .45 * sin(uEtin.z * (1.3 + 2.7 * e.w) + e.z * 9.);
        dehors += uEtC[i] * ((.25 + .45 * e.w) * uEtin.y * sc * bord
                             * (1.6 * exp(-d2 / t2) + .3 / (1. + d2 / (5. * t2))) * f * f);
      }
    }
    // L'ombre douce dessous, et la lumière du cœur que le verre y concentre.
    vec2 so = (p - vec2(0., 1.06)) / vec2(.6, .05), ca = (p - vec2(0., 1.055)) / vec2(.26, .032);
    ombre = .3 * exp(-1.4 * dot(so, so));
    dehors += uCoeurC * (uVie.z * .16 * exp(-1.5 * dot(ca, ca)));
  }
  vec3 cIn = versSrgb(aces(dedans * uExpo)), cOut = versSrgb(aces(dehors * uExpo));
  float aOut = clamp(ombre + uClair * max(cOut.r, max(cOut.g, cOut.b)) * .85, 0., 1.);
  float a = couv + aOut * (1. - couv);
  // Un grain imperceptible contre les bandes dans les dégradés sombres.
  float bruit = (fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(.06711056, .00583715)))) - .5) / 255.;
  sortie = vec4(max(cIn * couv + cOut * (1. - couv) + bruit * min(1., a * 4.), 0.), a);
}
"#;

/// Les uniformes de chaque programme, retrouvés par leur nom à la création.
const NOMS_LUMIERE: [&str; 8] = ["uFils", "uCentre", "uR", "uCoeur", "uCoeurC", "uOnde", "uPous", "uFlamme"];
const NOMS_DESCENTE: [&str; 3] = ["uSource", "uTexel", "uDest"];
const NOMS_MONTEE: [&str; 4] = ["uSource", "uAjout", "uTexel", "uDest"];
const NOMS_SCENE: [&str; 20] = [
    "uLumiere", "uEclat", "uCentre", "uCentreLu", "uTailleLu", "uR", "uRLu", "uExpo", "uClair", "uBloom", "uFenetre",
    "uTeinte", "uCoeurC", "uCiel", "uSol", "uCoeur", "uVie", "uEtin", "uEt", "uEtC",
];

/// Un source sans ses commentaires `//` (jusqu'au bout de la ligne) : GLSL n'a pas de chaîne où `//` voudrait dire
/// autre chose.
fn sans_commentaires(source: &str) -> String {
    let mut sortie = String::with_capacity(source.len());
    for ligne in source.lines() {
        sortie.push_str(ligne.split("//").next().unwrap_or("").trim_end());
        sortie.push('\n');
    }
    sortie
}

/// Ce qui précède chaque shader : la version que donne egui_glow (`#version 140` en OpenGL, `300 es` en OpenGL ES, et
/// alors la précision), puis les constantes de la page ; sans surfaces flottantes, la lumière s'encode sur 8 bits.
fn entete(version: ShaderVersion, hdr: bool) -> String {
    let mut e = version.version_declaration().to_string();
    if version.is_embedded() {
        e.push_str("precision highp float;\nprecision highp int;\nprecision highp sampler2D;\n");
    }
    e.push_str(&format!(
        "#define N_FILS {N}\n#define MORCEAUX {MORCEAUX}\n#define SEG {SEG}\n#define C_COULEUR {C_COULEUR}\n\
         #define C_TETE {C_TETE}\n#define C_BOITES {C_BOITES}\n#define ETINCELLES {ETINCELLES}\n\
         #define PORTEE {PORTEE:.4}\n#define LENTILLE {LENTILLE:.4}\n"
    ));
    if !hdr {
        e.push_str("#define ENCODAGE 1\n");
    }
    e
}

/// Un rendu tournant sur le processeur, reconnu à son nom (comme dans la page) : la lumière s'y fait moins fine.
fn logiciel(moteur: &str) -> bool {
    let m = moteur.to_lowercase();
    ["swiftshader", "basic render", "llvmpipe", "software"].iter().any(|n| m.contains(n))
}

struct Programme {
    p: glow::Program,
    u: HashMap<&'static str, glow::UniformLocation>,
}

impl Programme {
    fn u(&self, nom: &str) -> Option<&glow::UniformLocation> {
        self.u.get(nom)
    }
}

/// Une surface où dessiner : une texture de lumière et son framebuffer.
struct Surface {
    t: glow::Texture,
    f: glow::Framebuffer,
    l: i32,
}

/// Les ressources d'OpenGL de l'orbe : ses quatre programmes, la texture des données, ses surfaces.
struct Gl {
    lumiere: Programme,
    descente: Programme,
    montee: Programme,
    scene: Programme,
    vao: glow::VertexArray,
    fils: glow::Texture,
    noir: glow::Texture,
    /// Des surfaces en demi-flottants (la lumière sans limite, et son éclat) ; sinon sur 8 bits, encodées, sans éclat.
    hdr: bool,
    /// La taille de la lumière ; ses surfaces : la lumière, puis les quatre réductions et les trois remontées de
    /// l'éclat (en demi-flottants seulement).
    l: i32,
    surfaces: Vec<Surface>,
    /// La texture des données, en octets, prête à partir.
    octets: Vec<u8>,
}

/// Compile un programme (le sommet commun et un fragment) et retrouve ses uniformes.
///
/// # Safety
/// Sur le fil du rendu, le contexte courant (dans le rappel d'egui_glow).
unsafe fn compiler(
    gl: &glow::Context,
    entete: &str,
    embarque: bool,
    fragment: &str,
    noms: &[&'static str],
) -> Result<Programme, String> {
    // SAFETY : voir plus haut ; chaque objet créé est rendu en cas d'échec.
    unsafe {
        let p = gl.create_program()?;
        let mut shaders = Vec::new();
        for (sorte, source) in [(glow::VERTEX_SHADER, SOMMET), (glow::FRAGMENT_SHADER, fragment)] {
            let s = match gl.create_shader(sorte) {
                Ok(s) => s,
                Err(e) => {
                    gl.delete_program(p);
                    return Err(e);
                }
            };
            gl.shader_source(s, &format!("{entete}{}", sans_commentaires(source)));
            gl.compile_shader(s);
            if !gl.get_shader_compile_status(s) {
                let message = gl.get_shader_info_log(s);
                gl.delete_shader(s);
                for s in shaders {
                    gl.delete_shader(s);
                }
                gl.delete_program(p);
                return Err(format!("un shader ne compile pas : {}", message.trim()));
            }
            gl.attach_shader(p, s);
            shaders.push(s);
        }
        // La sortie unique va à la couleur 0 : GLSL 1.40 ne sait pas le dire dans le shader.
        if !embarque {
            gl.bind_frag_data_location(p, 0, "sortie");
        }
        gl.link_program(p);
        for s in shaders {
            gl.detach_shader(p, s);
            gl.delete_shader(s);
        }
        if !gl.get_program_link_status(p) {
            let message = gl.get_program_info_log(p);
            gl.delete_program(p);
            return Err(format!("un programme ne se lie pas : {}", message.trim()));
        }
        let u = noms.iter().filter_map(|n| gl.get_uniform_location(p, n).map(|l| (*n, l))).collect();
        Ok(Programme { p, u })
    }
}

/// Une texture 2D sans mipmap, bornée à ses bords.
///
/// # Safety
/// Sur le fil du rendu, le contexte courant.
unsafe fn texture(
    gl: &glow::Context,
    l: i32,
    h: i32,
    interne: u32,
    sorte: u32,
    filtre: u32,
    contenu: Option<&[u8]>,
) -> Result<glow::Texture, String> {
    // SAFETY : voir plus haut.
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            interne as i32,
            l,
            h,
            0,
            glow::RGBA,
            sorte,
            glow::PixelUnpackData::Slice(contenu),
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filtre as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filtre as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        Ok(t)
    }
}

/// Une surface de `l` × `l` pixels ; refusée si la carte ne sait pas y dessiner.
///
/// # Safety
/// Sur le fil du rendu, le contexte courant ; le framebuffer relié change (le rappel rend celui d'egui).
unsafe fn surface(gl: &glow::Context, l: i32, hdr: bool) -> Result<Surface, String> {
    // SAFETY : voir plus haut.
    unsafe {
        let (interne, sorte) = if hdr { (glow::RGBA16F, glow::HALF_FLOAT) } else { (glow::RGBA8, glow::UNSIGNED_BYTE) };
        let t = texture(gl, l, l, interne, sorte, glow::LINEAR, None)?;
        let f = match gl.create_framebuffer() {
            Ok(f) => f,
            Err(e) => {
                gl.delete_texture(t);
                return Err(e);
            }
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(t), 0);
        let etat = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if etat != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(f);
            gl.delete_texture(t);
            return Err(format!("une surface de {l} × {l} pixels est refusée ({etat:#06x})"));
        }
        Ok(Surface { t, f, l })
    }
}

/// Dessine dans une surface, toute entière.
///
/// # Safety
/// Sur le fil du rendu, le contexte courant.
unsafe fn passe(gl: &glow::Context, s: &Surface) {
    // SAFETY : voir plus haut.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(s.f));
        gl.viewport(0, 0, s.l, s.l);
    }
}

impl Gl {
    /// Les programmes, la texture des données et le reste, sans les surfaces (`allouer`).
    ///
    /// # Safety
    /// Sur le fil du rendu, le contexte courant.
    unsafe fn creer(gl: &glow::Context, version: ShaderVersion, hdr: bool) -> Result<Gl, String> {
        // SAFETY : voir plus haut.
        unsafe {
            let entete = entete(version, hdr);
            let mut faits = Vec::with_capacity(4);
            for (source, noms) in [
                (LUMIERE, &NOMS_LUMIERE[..]),
                (DESCENTE, &NOMS_DESCENTE[..]),
                (MONTEE, &NOMS_MONTEE[..]),
                (SCENE, &NOMS_SCENE[..]),
            ] {
                match compiler(gl, &entete, version.is_embedded(), source, noms) {
                    Ok(p) => faits.push(p),
                    Err(e) => {
                        for p in faits {
                            gl.delete_program(p.p);
                        }
                        return Err(e);
                    }
                }
            }
            let Ok([lumiere, descente, montee, scene]) = <[Programme; 4]>::try_from(faits) else {
                return Err("il manque un programme".into());
            };
            let vao = gl.create_vertex_array()?;
            gl.active_texture(glow::TEXTURE2);
            let fils = texture(gl, LARGEUR as i32, N as i32, glow::RGBA32F, glow::FLOAT, glow::NEAREST, None)?;
            gl.active_texture(glow::TEXTURE0);
            let noir = texture(gl, 1, 1, glow::RGBA8, glow::UNSIGNED_BYTE, glow::NEAREST, Some(&[0u8; 4][..]))?;
            // Les unités des textures : les données sur la 2, la lumière sur la 0, l'éclat (ou l'ajout) sur la 1.
            gl.use_program(Some(lumiere.p));
            gl.uniform_1_i32(lumiere.u("uFils"), 2);
            gl.use_program(Some(descente.p));
            gl.uniform_1_i32(descente.u("uSource"), 0);
            gl.use_program(Some(montee.p));
            gl.uniform_1_i32(montee.u("uSource"), 0);
            gl.uniform_1_i32(montee.u("uAjout"), 1);
            gl.use_program(Some(scene.p));
            gl.uniform_1_i32(scene.u("uLumiere"), 0);
            gl.uniform_1_i32(scene.u("uEclat"), 1);
            Ok(Gl {
                lumiere,
                descente,
                montee,
                scene,
                vao,
                fils,
                noir,
                hdr,
                l: 0,
                surfaces: Vec::new(),
                octets: Vec::with_capacity(LARGEUR * N * 16),
            })
        }
    }

    /// Les surfaces pour une lumière de `l` pixels : la lumière, puis (en demi-flottants) les quatre réductions de
    /// l'éclat et ses trois remontées.
    ///
    /// # Safety
    /// Sur le fil du rendu, le contexte courant.
    unsafe fn allouer(&mut self, gl: &glow::Context, l: i32) -> Result<(), String> {
        // SAFETY : voir plus haut.
        unsafe {
            self.liberer_surfaces(gl);
            self.l = l;
            let mut tailles = vec![l];
            if self.hdr {
                let mut t = l;
                for _ in 0..4 {
                    t = ((t + 1) / 2).max(1);
                    tailles.push(t);
                }
                for k in (1..=3).rev() {
                    tailles.push(tailles[k]);
                }
            }
            for t in tailles {
                self.surfaces.push(surface(gl, t, self.hdr)?);
            }
            Ok(())
        }
    }

    /// # Safety
    /// Sur le fil du rendu, le contexte courant.
    unsafe fn liberer_surfaces(&mut self, gl: &glow::Context) {
        // SAFETY : voir plus haut.
        unsafe {
            for s in self.surfaces.drain(..) {
                gl.delete_framebuffer(s.f);
                gl.delete_texture(s.t);
            }
        }
    }

    /// # Safety
    /// Sur le fil du rendu, le contexte courant.
    unsafe fn detruire(mut self, gl: &glow::Context) {
        // SAFETY : voir plus haut.
        unsafe {
            self.liberer_surfaces(gl);
            for p in [&self.lumiere, &self.descente, &self.montee, &self.scene] {
                gl.delete_program(p.p);
            }
            gl.delete_texture(self.fils);
            gl.delete_texture(self.noir);
            gl.delete_vertex_array(self.vao);
        }
    }

    /// Les trois passes d'une image : la lumière et son éclat dans les surfaces de l'orbe, puis le verre dans le
    /// framebuffer d'egui (`fb`), relié de nouveau avant la dernière passe.
    ///
    /// # Safety
    /// Sur le fil du rendu, le contexte courant (dans le rappel d'egui_glow).
    unsafe fn peindre(
        &mut self,
        gl: &glow::Context,
        im: &Image,
        info: &egui::PaintCallbackInfo,
        fb: Option<glow::Framebuffer>,
    ) -> Result<(), String> {
        // SAFETY : voir plus haut.
        unsafe {
            if im.l_lumiere != self.l || self.surfaces.is_empty() {
                self.allouer(gl, im.l_lumiere)?;
            }
            // Les courbes de l'image : la texture des données, sur l'unité 2.
            self.octets.clear();
            for v in &im.donnees {
                self.octets.extend_from_slice(&v.to_ne_bytes());
            }
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.fils));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                LARGEUR as i32,
                N as i32,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(&self.octets)),
            );
            // Les deux premières passes, dans les surfaces de l'orbe : ni découpe ni mélange ; un VAO vide, à soi.
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(Some(self.vao));
            let u = &im.u;
            let l = im.l_lumiere as f32;
            let r = l / 2.0 * SPHERE as f32;
            // 1. La lumière.
            passe(gl, &self.surfaces[0]);
            let p = &self.lumiere;
            gl.use_program(Some(p.p));
            gl.uniform_2_f32(p.u("uCentre"), l / 2.0, l / 2.0);
            gl.uniform_1_f32(p.u("uR"), r);
            gl.uniform_4_f32_slice(p.u("uCoeur"), &u.coeur);
            gl.uniform_3_f32_slice(p.u("uCoeurC"), &u.coeur_c);
            gl.uniform_4_f32_slice(p.u("uOnde"), &u.onde);
            gl.uniform_4_f32_slice(p.u("uPous"), &u.pous);
            gl.uniform_4_f32_slice(p.u("uFlamme"), &u.flamme);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            // 2. Son éclat : quatre réductions de moitié, trois remontées qui ajoutent chaque échelle — sauté quand
            // l'orbe est allégée (il s'est effacé avec les fils).
            gl.active_texture(glow::TEXTURE0);
            let flou = self.hdr && u.eclat_flou && self.surfaces.len() == 8;
            if flou {
                let p = &self.descente;
                gl.use_program(Some(p.p));
                for k in 1..5 {
                    let (source, s) = (&self.surfaces[k - 1], &self.surfaces[k]);
                    passe(gl, s);
                    gl.bind_texture(glow::TEXTURE_2D, Some(source.t));
                    gl.uniform_2_f32(p.u("uTexel"), 1.0 / source.l as f32, 1.0 / source.l as f32);
                    gl.uniform_2_f32(p.u("uDest"), s.l as f32, s.l as f32);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
                let p = &self.montee;
                gl.use_program(Some(p.p));
                for k in 0..3 {
                    let (source, ajout, s) = (&self.surfaces[4 + k], &self.surfaces[3 - k], &self.surfaces[5 + k]);
                    passe(gl, s);
                    gl.active_texture(glow::TEXTURE1);
                    gl.bind_texture(glow::TEXTURE_2D, Some(ajout.t));
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(source.t));
                    gl.uniform_2_f32(p.u("uTexel"), 1.0 / source.l as f32, 1.0 / source.l as f32);
                    gl.uniform_2_f32(p.u("uDest"), s.l as f32, s.l as f32);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
            }
            // 3. Le verre, dans le framebuffer d'egui : egui_glow ne le relie pas lui-même après un rappel. Le carré
            // de l'orbe, en pixels de la fenêtre (le bas compte depuis le bas), sans le rogner au bord de l'écran :
            // la découpe d'egui (le scissor) fait ce travail.
            gl.bind_framebuffer(glow::FRAMEBUFFER, fb);
            let ppp = info.pixels_per_point;
            let gauche = (info.viewport.min.x * ppp).round() as i32;
            let haut = (info.viewport.min.y * ppp).round() as i32;
            let bas = info.screen_size_px[1] as i32 - haut - im.l_ecran;
            gl.viewport(gauche, bas, im.l_ecran, im.l_ecran);
            gl.enable(glow::SCISSOR_TEST);
            // Le mélange d'egui, l'alpha prémultiplié de la page : le halo et les étincelles s'ajoutent au fond.
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func_separate(glow::ONE, glow::ONE_MINUS_SRC_ALPHA, glow::ONE_MINUS_DST_ALPHA, glow::ONE);
            let p = &self.scene;
            gl.use_program(Some(p.p));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(if flou { self.surfaces[7].t } else { self.noir }));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.surfaces[0].t));
            let le = im.l_ecran as f32;
            // gl_FragCoord compte depuis le coin de la fenêtre : le centre se décale de l'origine du carré.
            gl.uniform_2_f32(p.u("uCentre"), gauche as f32 + le / 2.0, bas as f32 + le / 2.0);
            gl.uniform_1_f32(p.u("uR"), le / 2.0 * SPHERE as f32);
            gl.uniform_2_f32(p.u("uCentreLu"), l / 2.0, l / 2.0);
            gl.uniform_1_f32(p.u("uRLu"), r);
            gl.uniform_2_f32(p.u("uTailleLu"), l, l);
            gl.uniform_1_f32(p.u("uExpo"), 1.0);
            gl.uniform_1_f32(p.u("uClair"), u.clair);
            gl.uniform_1_f32(p.u("uBloom"), if flou { u.bloom } else { 0.0 });
            gl.uniform_1_f32(p.u("uFenetre"), u.fenetre);
            gl.uniform_3_f32_slice(p.u("uCiel"), &u.ciel);
            gl.uniform_3_f32_slice(p.u("uSol"), &u.sol);
            gl.uniform_3_f32_slice(p.u("uTeinte"), &u.teinte);
            gl.uniform_3_f32_slice(p.u("uCoeurC"), &u.coeur_c);
            gl.uniform_4_f32_slice(p.u("uCoeur"), &u.coeur);
            gl.uniform_4_f32_slice(p.u("uVie"), &u.vie);
            gl.uniform_4_f32_slice(p.u("uEtin"), &u.etin);
            gl.uniform_4_f32_slice(p.u("uEt"), &u.et);
            gl.uniform_3_f32_slice(p.u("uEtC"), &u.couleurs_et);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            match gl.get_error() {
                glow::NO_ERROR => Ok(()),
                e => Err(format!("OpenGL signale l'erreur {e:#06x}")),
            }
        }
    }
}

/// Prépare OpenGL pour l'orbe, la première fois : GLSL 1.40 (OpenGL 3.1) ou GLSL ES 3.00 ; des surfaces en
/// demi-flottants, sinon sur 8 bits. Rend aussi le nom de la carte et sa version, pour le journal.
///
/// # Safety
/// Sur le fil du rendu, le contexte courant (dans le rappel d'egui_glow).
unsafe fn ouvrir(gl: &glow::Context, l: i32) -> Result<(Gl, String), String> {
    // SAFETY : voir plus haut.
    unsafe {
        let moteur =
            format!("{} — {}", gl.get_parameter_string(glow::RENDERER), gl.get_parameter_string(glow::VERSION));
        let version = ShaderVersion::get(gl);
        if !matches!(version, ShaderVersion::Gl140 | ShaderVersion::Es300) {
            return Err(format!("{moteur} : il faut OpenGL 3.1 ou OpenGL ES 3.0"));
        }
        let hdr = !version.is_embedded() || gl.supported_extensions().contains("GL_EXT_color_buffer_float");
        let mut g = Gl::creer(gl, version, hdr)?;
        if let Err(e) = g.allouer(gl, l) {
            g.detruire(gl);
            if !hdr {
                return Err(e);
            }
            // Sans surfaces flottantes : la lumière tient sur 8 bits, encodée, sans éclat (le repli de la page).
            g = Gl::creer(gl, version, false)?;
            if let Err(e2) = g.allouer(gl, l) {
                g.detruire(gl);
                return Err(format!("{e} ; {e2}"));
            }
        }
        Ok((g, moteur))
    }
}

/// Le rappel d'une image, sur le fil du rendu : prépare OpenGL la première fois, dessine, et rend à egui son
/// framebuffer quoi qu'il arrive. Une erreur, ou même une panique, éteint l'orbe — jamais la fenêtre.
fn rappel(partage: &Mutex<Partage>, im: &Image, info: &egui::PaintCallbackInfo, painter: &egui_glow::Painter) {
    let gl: &glow::Context = painter.gl();
    let Ok(mut p) = partage.lock() else { return };
    // SAFETY : egui_glow appelle ce rappel sur le fil du rendu, son contexte courant.
    let fb = unsafe { gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING) };
    // Les erreurs d'avant ne sont pas les nôtres.
    for _ in 0..16 {
        // SAFETY : voir plus haut.
        if unsafe { gl.get_error() } == glow::NO_ERROR {
            break;
        }
    }
    if matches!(p.rendu, Rendu::AFaire) {
        // SAFETY : voir plus haut.
        p.rendu = match catch_unwind(AssertUnwindSafe(|| unsafe { ouvrir(gl, im.l_lumiere) })) {
            Ok(Ok((g, moteur))) => {
                if logiciel(&moteur) {
                    p.qualite = 0.6;
                }
                let surfaces = if g.hdr { "en demi-flottants, avec son éclat" } else { "sur 8 bits, sans éclat" };
                journal::ecrire(&format!("L'orbe se dessine par OpenGL ({moteur}) : sa lumière {surfaces}."));
                Rendu::Pret(Box::new(g))
            }
            Ok(Err(e)) => {
                journal::ecrire(&format!("L'orbe ne se dessine pas ({e}) : un disque la remplace."));
                Rendu::EnPanne
            }
            Err(_) => {
                journal::ecrire("L'orbe ne se dessine pas (une panique en préparant OpenGL) : un disque la remplace.");
                Rendu::EnPanne
            }
        };
    }
    let mesurer = p.mesurer;
    let mut echec = None;
    let mut duree = None;
    if let Rendu::Pret(g) = &mut p.rendu {
        // SAFETY : voir plus haut.
        let debut = mesurer.then(|| {
            unsafe { gl.finish() };
            Instant::now()
        });
        let r = catch_unwind(AssertUnwindSafe(|| unsafe { g.peindre(gl, im, info, fb) }));
        if let Some(d) = debut {
            // SAFETY : voir plus haut.
            unsafe { gl.finish() };
            duree = Some(d.elapsed().as_secs_f64() * 1000.0);
        }
        echec = match r {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(_) => Some("une panique dans le dessin".to_string()),
        };
    }
    if let Some(d) = duree {
        p.durees.push(d);
        let trop = p.durees.len().saturating_sub(90);
        p.durees.drain(..trop);
    }
    if let Some(e) = echec {
        journal::ecrire(&format!("L'orbe ne se dessine plus ({e}) : un disque la remplace."));
        if let Rendu::Pret(g) = std::mem::replace(&mut p.rendu, Rendu::EnPanne) {
            // SAFETY : voir plus haut.
            unsafe { g.detruire(gl) };
        }
    }
    // Ce qu'egui avait relié lui revient, quoi qu'il soit arrivé ; il remet lui-même le reste (son programme, son
    // VAO, le mélange, la découpe, le viewport).
    // SAFETY : voir plus haut.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, fb);
        gl.bind_vertex_array(None);
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Avance la vie de `secondes`, par images de `pas`.
    fn vivre(v: &mut Vie, secondes: f64, pas: f64, depuis: f64) -> f64 {
        let mut t = depuis;
        while t < depuis + secondes {
            t += pas;
            v.avancer(pas, pas.min(PAS_MAX), t);
        }
        t
    }

    #[test]
    fn chaque_etat_a_son_apparence_et_un_inconnu_vaut_pret() {
        assert_eq!(etat_connu("reflexion").1.vitesse, 2.1);
        assert_eq!(etat_connu("confirmation").1.teinte, [245.0, 158.0, 11.0]);
        assert_eq!(etat_connu("erreur").1.eclat, 0.35);
        assert_eq!(etat_connu("").0, "pret");
        assert_eq!(etat_connu("n'importe quoi"), etat_connu("pret"));
        // La plus vive, la plus agitée : la réflexion ; la plus calme : l'erreur.
        let vitesses: Vec<f64> = ETATS.iter().map(|(_, a)| a.vitesse).collect();
        assert_eq!(vitesses.iter().copied().fold(0.0, f64::max), 2.1);
        assert_eq!(vitesses.iter().copied().fold(9.0, f64::min), 0.14);
    }

    #[test]
    fn le_premier_etat_se_pose_d_emblee_puis_les_autres_glissent() {
        let mut v = Vie::nouvelle(7);
        v.poser("reflexion", 0.0, 0.0);
        assert_eq!(v.cur, etat_connu("reflexion").1, "rien n'est encore affiché : pas de transition");
        v.deja_vu = true;
        v.poser("pret", 0.0, 0.0);
        let t = vivre(&mut v, 0.2, 1.0 / 30.0, 0.0);
        // En 0,2 s, k = 1 − e^(−2,6 × 0,2) : un peu moins de la moitié du chemin.
        assert!(v.cur.vitesse > 1.2 && v.cur.vitesse < 1.6, "elle glisse, elle ne saute pas : {}", v.cur.vitesse);
        vivre(&mut v, 4.0, 1.0 / 30.0, t);
        assert!((v.cur.vitesse - 0.42).abs() < 0.004, "posée en quatre secondes : {}", v.cur.vitesse);
    }

    #[test]
    fn la_courbe_d_un_fil() {
        let mut v = Vie::nouvelle(42);
        v.poser("pret", 0.0, 0.0);
        v.deja_vu = true;
        vivre(&mut v, 1.0, 1.0 / 30.0, 0.0);
        v.preparer(300.0);
        for i in 0..N {
            let ligne = &v.donnees[i * LARGEUR * 4..(i + 1) * LARGEUR * 4];
            let point = |q: usize| &ligne[q * 4..q * 4 + 4];
            for q in 0..POINTS {
                let [x, y, intensite, inverse] = [point(q)[0], point(q)[1], point(q)[2], point(q)[3]];
                assert!(x.is_finite() && y.is_finite(), "fil {i}, point {q}");
                // Dans la sphère, la perspective comprise (0,97 × 0,9 / 0,8), le regard en plus.
                assert!(x.hypot(y) < 1.2, "fil {i}, point {q} : {x}, {y}");
                assert!(intensite >= 0.0 && inverse > 0.0);
                // Chaque point est dans la boîte de son morceau (ou des deux, à la jointure).
                let m = (q.saturating_sub(1) / SEG).min(MORCEAUX - 1);
                let b = &ligne[(C_BOITES + m) * 4..(C_BOITES + m) * 4 + 4];
                assert!(x >= b[0] && y >= b[1] && x <= b[2] && y <= b[3], "fil {i}, point {q} hors de sa boîte");
            }
            // La queue s'éteint, la tête brille : la tête est le dernier point, en plus vif.
            assert_eq!(point(0)[2], 0.0, "la queue d'un fil ne luit pas");
            let tete = &ligne[C_TETE * 4..C_TETE * 4 + 4];
            assert_eq!((tete[0], tete[1]), (point(POINTS - 1)[0], point(POINTS - 1)[1]));
            assert!(tete[2] >= point(POINTS - 1)[2] && tete[3] < point(POINTS - 1)[3], "la tête, plus large");
            assert_eq!(ligne[C_COULEUR * 4 + 3], 1.0);
        }
    }

    #[test]
    fn allegee_elle_ne_garde_qu_un_fil_sur_deux() {
        let mut v = Vie::nouvelle(3);
        v.poser("transcription", 0.0, 0.0);
        v.deja_vu = true;
        v.preparer(300.0);
        for i in 0..N {
            let boite = &v.donnees[(i * LARGEUR + C_BOITES) * 4..(i * LARGEUR + C_BOITES) * 4 + 4];
            if i % 2 == 1 {
                assert_eq!(boite, [9.0, 9.0, -9.0, -9.0], "le fil {i} n'est plus calculé");
            } else {
                assert!(boite[0] < boite[2], "le fil {i} reste");
            }
        }
        assert!(!v.uniformes(300.0, false).eclat_flou, "allégée, sans éclat");
    }

    #[test]
    fn une_impulsion_court_le_long_d_un_fil_puis_s_eteint() {
        let mut v = Vie::nouvelle(5);
        v.poser("pret", 0.0, 0.0);
        v.deja_vu = true;
        v.lancer_pouls(1);
        assert_eq!(v.pouls.iter().filter(|p| **p < 5.0).count(), 1);
        assert!(v.s_anime(100.0), "une impulsion en route : elle s'anime");
        vivre(&mut v, 2.0, 1.0 / 30.0, 0.0);
        assert!(v.pouls.iter().all(|p| *p == 9.0), "arrivée à la tête, elle s'éteint");
        // En réflexion, les impulsions se suivent.
        v.poser("reflexion", 0.0, 2.0);
        vivre(&mut v, 1.0, 1.0 / 30.0, 2.0);
        assert!(v.pouls.iter().any(|p| *p < 5.0));
    }

    #[test]
    fn elle_somnole_apres_quatre_minutes_et_un_rien_la_reveille() {
        let mut v = Vie::nouvelle(11);
        v.poser("pret", 0.0, 0.0);
        v.deja_vu = true;
        let t = vivre(&mut v, SOMMEIL + 20.0, 0.25, 0.0);
        assert!(v.vivant.veille > 0.95, "endormie : {}", v.vivant.veille);
        assert!(v.vue.eclat < 0.8 * 0.5, "elle pâlit en dormant : {}", v.vue.eclat);
        v.poser("ecoute", 0.5, t);
        assert!(v.vivant.onde > 0.9, "réveillée, une onde part de son cœur");
        vivre(&mut v, 2.0, 1.0 / 30.0, t);
        assert!(v.vivant.veille < 0.01, "réveillée vite : {}", v.vivant.veille);
    }

    #[test]
    fn au_repos_et_posee_elle_ne_s_anime_pas() {
        let mut v = Vie::nouvelle(13);
        v.poser("pret", 0.0, 0.0);
        v.deja_vu = true;
        v.prochaine_pensee = 1e9;
        let t = vivre(&mut v, 3.0, 0.25, 0.0);
        assert!(!v.s_anime(t), "au repos : la cadence du repos");
        v.poser("ecoute", 0.4, t);
        assert!(v.s_anime(t), "elle écoute");
        let t = vivre(&mut v, 3.0, 1.0 / 30.0, t);
        v.poser("pret", 0.0, t);
        assert!(v.s_anime(t), "la transition glisse encore");
        // La souris qui bouge, puis qui s'arrête.
        let t = vivre(&mut v, 5.0, 1.0 / 30.0, t);
        v.pointeur(3.0, 3.0, t);
        assert!(v.s_anime(t + 1.0));
        assert!(!v.s_anime(t + 3.0), "la souris loin d'elle et immobile depuis 3 s");
        // Toucher : un élan, une onde, trois impulsions.
        v.toucher(0.1, 0.2);
        assert!(v.vivant.elan == 1.0 && v.vivant.onde == 1.0);
        assert_eq!(v.pouls.iter().filter(|p| **p < 5.0).count(), 3);
    }

    #[test]
    fn en_ecoute_la_voix_gonfle_les_fils_et_le_coeur() {
        let mut v = Vie::nouvelle(17);
        v.poser("ecoute", 0.0, 0.0);
        v.deja_vu = true;
        vivre(&mut v, 0.1, 1.0 / 30.0, 0.0);
        let calme = v.uniformes(300.0, false).coeur[2];
        v.poser("ecoute", 1.0, 0.1);
        vivre(&mut v, 1.0 / 30.0, 1.0 / 30.0, 0.1);
        assert_eq!(v.vue.vie, 1.0);
        assert!(v.uniformes(300.0, false).coeur[2] > calme + 0.15, "le cœur suit la voix");
    }

    #[test]
    fn le_souffle_et_le_tone_mapping() {
        assert!((0..10_000).all(|i| souffle(i as f64 * 0.01).abs() <= 1.0));
        // ACES : noir reste noir, la lumière la plus vive ne dépasse jamais le blanc, et l'ordre est gardé.
        assert_eq!(aces(0.0), 0.0);
        assert!(aces(100.0) <= 1.0 && aces(100.0) > 0.99);
        assert!((1..400).all(|i| aces(i as f64 * 0.01) > aces((i - 1) as f64 * 0.01)));
        assert!((aces(0.18) - 0.2672).abs() < 1e-3, "le gris moyen : {}", aces(0.18));
        // Le sRGB, aller et retour.
        assert_eq!(vers_srgb(0.0), 0.0);
        assert!((vers_srgb(1.0) - 1.0).abs() < 1e-9);
        for c in [0.0, 10.0, 128.0, 200.0, 255.0] {
            assert!((vers_srgb(lin(c)) * 255.0 - c).abs() < 1e-6, "{c}");
        }
    }

    #[test]
    fn les_shaders_sans_commentaires_sont_en_ascii_et_portent_leurs_uniformes() {
        for (source, noms) in [
            (LUMIERE, &NOMS_LUMIERE[..]),
            (DESCENTE, &NOMS_DESCENTE[..]),
            (MONTEE, &NOMS_MONTEE[..]),
            (SCENE, &NOMS_SCENE[..]),
        ] {
            let s = sans_commentaires(source);
            assert!(s.is_ascii(), "un caractère accentué reste hors des commentaires");
            assert!(s.contains("out vec4 sortie;") && !s.contains("#version") && !s.contains("precision"));
            for n in noms {
                assert!(s.contains(&format!(" {n}")) || s.contains(&format!(", {n}")), "{n} manque à son shader");
            }
        }
        assert!(sans_commentaires(SOMMET).is_ascii());
        let e = entete(ShaderVersion::Gl140, true);
        assert!(e.starts_with("#version 140\n") && e.contains("#define PORTEE 0.0850\n") && !e.contains("ENCODAGE"));
        let e = entete(ShaderVersion::Es300, false);
        assert!(e.starts_with("#version 300 es\nprecision highp float;") && e.contains("#define ENCODAGE 1\n"));
        assert!(logiciel("llvmpipe (LLVM 15.0.7, 256 bits)") && !logiciel("NVIDIA GeForce RTX 3060/PCIe/SSE2"));
    }
}
