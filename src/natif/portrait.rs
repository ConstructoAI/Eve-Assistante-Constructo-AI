//! Le portrait d'Eve, dans le thème « Constructo AI » (demandé le 25 octobre : « une photo IA (EVE) à la place de
//! l'orbe », puis « avec un fond transparent ») : une image que la personne choisit, à la place de l'orbe — dans la
//! bannière du tableau de bord, en tête de la conversation seule, dans l'en-tête du dock et devant les bulles d'Eve —,
//! cernée d'un anneau qui dit ce qu'elle fait, comme l'orbe le montrait. Une photo se montre en disque ; un portrait
//! détouré (un PNG au fond transparent) se pose sur un disque clair, comme sous la lumière d'un studio, et en grand, sa
//! tête sort du cercle par le haut. Sa seule source : `portrait.png` (ou `.jpg`, `.jpeg`, `.webp`, `.gif`) du dossier
//! local, que les réglages posent ou retirent et que la fenêtre relit quand il change. Sans lui, l'orbe reste.
//!
//! Ses poses l'animent (« elle va bouger ses lèvres et yeux ? », « je veux vraiment qu'elle soit humaine, le plus réaliste
//! possible », « sa bouche pourrait-elle plus humaine… plus de frames », « elle semble avoir toujours les dents
//! serrées », « sa tête qui est toujours fixe », « des émotions triste, empathie, drôle… »), si elles sont là, de la
//! même taille que la photo : les yeux qui se ferment (`portrait-yeux-1`, `-2`…), des regards de côté
//! (`portrait-regard-1`…), les sourcils levés (`portrait-sourcils-1`…), la bouche — ouverte, la mâchoire de plus en plus
//! basse (`portrait-bouche-1`…), de même sans sourire (`portrait-detendu-1`…), ronde (`portrait-rond-1`…), étirée
//! (`portrait-large-1`…), et les lèvres closes (`portrait-ferme-1`) —, et une pose par émotion (`portrait-emotion-joie`…).
//! Chacune devient un calque — ce qui y change par rapport à la photo, le reste transparent —, et les calques se posent
//! l'un sur l'autre, en fondu : en grand, elle cligne et regarde de côté de temps en temps, même en parlant ; sa bouche
//! suit sa voix et la forme de ses syllabes, lues à chaque image ([`crate::voix::bouche_du_moment`]) ; ses sourcils se
//! lèvent sur une syllabe appuyée ; elle respire ; sa tête bouge, penche, hoche avec sa voix ; son visage prend
//! l'émotion de sa réponse, que Claude choisit ([`Emotion`]).

use std::f32::consts::{FRAC_PI_4, PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2, vec2};
use eve::parole::{Emotion, Forme};

use super::theme::erp;

/// Les extensions que peut porter le portrait, et chacune de ses poses.
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];
/// Les sortes de poses (`portrait-yeux-1`, `portrait-regard-1`, `portrait-bouche-1`…), douze au plus chacune : les yeux
/// qui se ferment, les regards ; puis la bouche — ouverte (a, è), ronde (o, ou, u), étirée (i, é), chacune de plus en
/// plus ouverte, et les lèvres closes (m, b, p) ; les sourcils, de plus en plus levés ; la bouche ouverte d'un sourire
/// détendu (pour une émotion grave). Et, à part, une pose par émotion (`portrait-emotion-triste`…).
const SORTES: [&str; 8] = ["yeux", "regard", "bouche", "rond", "large", "ferme", "sourcils", "detendu"];
const POSES_MAX: usize = 12;
/// Le grand côté du portrait chargé, en pixels : de quoi la bannière d'un écran 4K.
const COTE: usize = 768;
/// Ce que la tête d'un portrait détouré dépasse du cercle, en grand : une part du diamètre.
const DEPASSE: f32 = 0.10;
/// Le visage de plus près, dans les petits disques : le grossissement, et la hauteur du visage dans le disque de la
/// grande vue (une part de son diamètre, depuis son haut).
const DE_PRES: f32 = 1.6;
const HAUTEUR_DU_VISAGE: f32 = 0.3;

/// Le portrait prêt à peindre : sa photo ; les calques de ses poses ([`calque`]), une liste par [sorte](SORTES) — les
/// yeux de plus en plus fermés, les regards, la bouche de plus en plus ouverte ; détouré, le fond de son disque ; le carré
/// de l'image (en coordonnées de texture, de 0 à 1) que couvre le disque de la grande vue.
#[derive(Clone)]
pub(crate) struct Visage {
    texture: egui::TextureHandle,
    calques: [Vec<egui::TextureHandle>; 8],
    emotions: Vec<(Emotion, egui::TextureHandle)>,
    fond: Option<egui::TextureHandle>,
    cadre: Rect,
}

/// Ce que rend le fil de lecture : la photo et les calques de ses poses et de ses émotions, détourée ou non, le carré de
/// son disque.
struct Images {
    photo: egui::ColorImage,
    calques: [Vec<egui::ColorImage>; 8],
    emotions: Vec<(Emotion, egui::ColorImage)>,
    detoure: bool,
    cadre: Rect,
}

/// La pose du visage en grand à un instant ([`Portrait::pose_du_moment`]) : les calques à poser sur la photo, chacun
/// avec sa part (de 0 à 1) — l'émotion, la bouche d'en dessous, celle d'au-dessus qui se fond en elle, les lèvres
/// closes, les sourcils (de même), puis les yeux —, sa tête, son souffle (ce dont elle grandit en inspirant, une part de
/// sa taille), et dans combien de secondes la redessiner.
#[derive(Clone, Copy, Default)]
pub(crate) struct Pose {
    emotion: Option<(egui::TextureId, f32)>,
    bouche: [Option<(egui::TextureId, f32)>; 3],
    sourcils: [Option<(egui::TextureId, f32)>; 2],
    yeux: Option<egui::TextureId>,
    tete: Tete,
    souffle: f32,
    pub(crate) apres: f64,
}

/// Où est la tête d'Eve, ses épaules ne bougeant pas : penchée (en radians ; positif, vers son épaule gauche, à droite à
/// l'écran) et déplacée (une part du rayon du disque, en x et en y, y vers le bas).
#[derive(Clone, Copy, Default)]
struct Tete {
    roulis: f32,
    x: f32,
    y: f32,
}

/// Ce que fait le visage, d'une image à l'autre (les heures sont celles de la fenêtre, en secondes) : le prochain
/// clignement ; le regard de côté et jusqu'à quand, le prochain ; l'ouverture de la bouche et ses lèvres closes, lissées,
/// la forme de sa syllabe ; l'élan de la voix (lissé plus lentement, pour la tête) et l'animation de la parole ; les
/// sourcils (levés de 0 à 1, lissés), jusqu'à quand, et quand ils pourront se lever de nouveau ; l'émotion montrée et sa
/// part, le penchant qu'elle donne à la tête (lissé) ; le dernier calcul ; le hasard.
#[derive(Default)]
struct Mimique {
    clin: f64,
    regard: Option<usize>,
    regard_jusqua: f64,
    prochain_regard: f64,
    ouverture: f32,
    levres_closes: f32,
    forme: Option<Forme>,
    elan: f32,
    animation: f32,
    sourcils: f32,
    sourcils_jusqua: f64,
    prochains_sourcils: f64,
    emotion: Option<Emotion>,
    emotion_part: f32,
    biais: Tete,
    vu_a: f64,
    hasard: u64,
}

impl Mimique {
    /// Un nombre de 0 à 1, différent à chaque fois (un xorshift) : le visage ne bouge pas comme un métronome.
    fn hasard(&mut self) -> f64 {
        let mut x = if self.hasard == 0 { 0x9E37_79B9_7F4A_7C15 } else { self.hasard };
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.hasard = x;
        (x % 10_000) as f64 / 10_000.0
    }
}

/// Le portrait d'Eve, tel que la fenêtre le tient : son dossier local, ses fichiers vus (leur date et leur taille), le
/// visage prêt à peindre, sa lecture en cours, sa mimique.
pub(crate) struct Portrait {
    local: PathBuf,
    vu: Vec<(PathBuf, SystemTime, u64)>,
    visage: Option<Visage>,
    lu: Arc<Mutex<Option<Result<Images, String>>>>,
    en_lecture: bool,
    regarde_a: f64,
    mimique: Mimique,
}

impl Portrait {
    pub(crate) fn nouveau(local: PathBuf) -> Portrait {
        Portrait {
            local,
            vu: Vec::new(),
            visage: None,
            lu: Arc::default(),
            en_lecture: false,
            regarde_a: f64::NEG_INFINITY,
            mimique: Mimique::default(),
        }
    }

    /// Le portrait à cette image : ses fichiers regardés toutes les deux secondes, relus dans un fil quand l'un d'eux
    /// change, avec sa mise en place ([`mise_en_place`]). `None` : pas de portrait (ou illisible, et le journal le dit) :
    /// l'orbe reste.
    pub(crate) fn visage(&mut self, ctx: &egui::Context) -> Option<Visage> {
        let maintenant = ctx.input(|i| i.time);
        if let Some(lu) = self.lu.lock().ok().and_then(|mut l| l.take()) {
            self.en_lecture = false;
            self.visage = match lu {
                Ok(i) => {
                    let texture = |nom: String, image| ctx.load_texture(nom, image, egui::TextureOptions::LINEAR);
                    let mut sortes = SORTES.iter();
                    let calques = i.calques.map(|images| {
                        let sorte = sortes.next().copied().unwrap_or("");
                        let nom = |k: usize| format!("portrait-{sorte}-{k}");
                        images.into_iter().enumerate().map(|(k, im)| texture(nom(k), im)).collect()
                    });
                    let emotions = i
                        .emotions
                        .into_iter()
                        .map(|(e, im)| (e, texture(format!("portrait-emotion-{}", e.nom()), im)))
                        .collect();
                    Some(Visage {
                        texture: texture("portrait-d-eve".into(), i.photo),
                        calques,
                        emotions,
                        fond: i.detoure.then(|| fond_de_studio(ctx)),
                        cadre: i.cadre,
                    })
                }
                Err(e) => {
                    crate::journal::ecrire(&format!("le portrait d'Eve ne se lit pas : {e}"));
                    None
                }
            };
        }
        if !self.en_lecture && maintenant - self.regarde_a >= 2.0 {
            self.regarde_a = maintenant;
            let vus: Vec<_> = fichiers(&self.local)
                .into_iter()
                .filter_map(|c| {
                    let m = std::fs::metadata(&c).ok()?;
                    Some((c, m.modified().ok()?, m.len()))
                })
                .collect();
            if vus != self.vu {
                self.vu = vus;
                if self.vu.is_empty() {
                    self.visage = None;
                } else {
                    self.en_lecture = true;
                    let (lu, ctx, local) = (self.lu.clone(), ctx.clone(), self.local.clone());
                    std::thread::spawn(move || {
                        let images = lire(&local);
                        if let Ok(mut l) = lu.lock() {
                            *l = Some(images);
                        }
                        ctx.request_repaint();
                    });
                }
            }
        }
        self.visage.clone()
    }

    /// La pose du visage en grand à l'instant `temps` (celui de la fenêtre) : quand Eve `parle`, sa bouche suit sa voix
    /// à chaque image ([`crate::voix::bouche_du_moment`]) — ouverte, ronde ou étirée selon la syllabe, plus ou moins
    /// selon le niveau, d'une pose à la suivante en fondu, les lèvres closes avant m, b, p ; toutes les deux à six
    /// secondes, elle cligne (parfois deux fois de suite) ; de temps en temps, elle regarde de côté un moment. `vivant` :
    /// les animations de Windows sont permises — sans elles, ni souffle ni regard de côté, et elle cligne encore.
    /// `emotion` : celle de la réponse ([`Emotion`]), que le visage prend en fondu (sa pose, et un penchant de la tête) ;
    /// pour une émotion grave, elle parle d'un sourire détendu.
    pub(crate) fn pose_du_moment(
        &mut self,
        temps: f64,
        parle: bool,
        vivant: bool,
        emotion: Option<Emotion>,
    ) -> Option<Pose> {
        let v = self.visage.as_ref()?;
        let [yeux, regards, ouvertes, rondes, etirees, fermees, sourcils, detendues] = &v.calques;
        let m = &mut self.mimique;
        let dt = (temps - m.vu_a).clamp(0.0, 0.1) as f32;
        m.vu_a = temps;
        let mut pose = Pose::default();
        let suivre = |de: f32, vers: f32, tau: f32| de + (vers - de) * (1.0 - (-dt / tau).exp());
        // L'émotion : celle d'avant s'efface (200 ms) avant que la nouvelle ne vienne (350 ms) ; sans sa pose, rien.
        let voulue = emotion.filter(|e| v.emotions.iter().any(|(f, _)| f == e));
        if m.emotion != voulue && m.emotion.is_some() {
            m.emotion_part = suivre(m.emotion_part, 0.0, 0.2);
            if m.emotion_part < 0.02 {
                (m.emotion, m.emotion_part) = (voulue, 0.0);
            }
        } else {
            m.emotion = voulue;
            m.emotion_part = suivre(m.emotion_part, if voulue.is_some() { 1.0 } else { 0.0 }, 0.35);
        }
        pose.emotion = m
            .emotion
            .and_then(|e| v.emotions.iter().find(|(f, _)| *f == e))
            .filter(|_| m.emotion_part > 0.01)
            .map(|(_, t)| (t.id(), m.emotion_part));
        // L'émotion arrivée (ou partie) : sans les animations de Windows, rien d'autre ne presse la prochaine image.
        let emotion_posee =
            m.emotion == voulue && (m.emotion_part - if voulue.is_some() { 1.0 } else { 0.0 }).abs() < 0.01;
        let grave =
            matches!(m.emotion, Some(Emotion::Empathie | Emotion::Triste | Emotion::Reflexion | Emotion::Serieuse));
        // La bouche : le niveau de la voix qui s'entend, et la forme de sa syllabe ; elle s'ouvre vite (25 ms), se
        // referme un peu moins vite (70 ms), les lèvres se closent et se rouvrent en 30 ms.
        let (cible, forme) = if parle { crate::voix::bouche_du_moment() } else { None }.unzip();
        let cible = cible.unwrap_or(0.0);
        m.ouverture = suivre(m.ouverture, cible, if cible > m.ouverture { 0.025 } else { 0.07 });
        let closes = forme == Some(Forme::Fermee);
        m.levres_closes = suivre(m.levres_closes, if closes { 1.0 } else { 0.0 }, 0.03);
        if let Some(f) = forme.filter(|&f| f != Forme::Fermee) {
            m.forme = Some(f);
        }
        let famille = match m.forme {
            Some(Forme::Ronde) if !rondes.is_empty() => rondes,
            Some(Forme::Etiree) if !etirees.is_empty() => etirees,
            _ if grave && !detendues.is_empty() => detendues,
            _ => ouvertes,
        };
        let (k, part) = ouverture_en_poses(m.ouverture, famille.len());
        pose.bouche = [
            k.checked_sub(1).map(|i| (famille[i].id(), 1.0)),
            (k < famille.len() && part > 0.01).then(|| (famille[k].id(), part)),
            fermees.first().filter(|_| m.levres_closes > 0.01).map(|t| (t.id(), m.levres_closes)),
        ];
        // Les sourcils : levés une demi-seconde sur une syllabe appuyée (au plus toutes les deux à cinq secondes), et de
        // loin en loin au repos (toutes les quinze à quarante secondes) ; ils montent en 80 ms, redescendent en 200.
        if vivant && !sourcils.is_empty() {
            if m.prochains_sourcils == 0.0 {
                m.prochains_sourcils = temps + 12.0;
            }
            if temps >= m.prochains_sourcils && (!parle || m.ouverture > 0.8) {
                m.sourcils_jusqua = temps + 0.5;
                let attente = if parle { 2.0 + 3.0 * m.hasard() } else { 15.0 + 25.0 * m.hasard() };
                m.prochains_sourcils = temps + attente;
            }
        }
        let leves = temps < m.sourcils_jusqua;
        m.sourcils = suivre(m.sourcils, if leves { 1.0 } else { 0.0 }, if leves { 0.08 } else { 0.2 });
        let x = m.sourcils.clamp(0.0, 1.0) * sourcils.len() as f32;
        let k = (x.floor() as usize).min(sourcils.len());
        pose.sourcils = [
            k.checked_sub(1).map(|i| (sourcils[i].id(), 1.0)),
            (k < sourcils.len() && x - k as f32 > 0.01).then(|| (sourcils[k].id(), x - k as f32)),
        ];
        // Les yeux : le clignement, sinon le regard.
        if m.clin == 0.0 {
            m.clin = temps + 2.0;
        }
        if temps - m.clin >= DUREE_DU_CLIN {
            m.clin = temps + if m.hasard() < 0.12 { 0.3 } else { 2.0 + 4.0 * m.hasard() };
        }
        if vivant && !regards.is_empty() {
            if m.prochain_regard == 0.0 {
                m.prochain_regard = temps + 4.0;
            }
            if m.regard.is_some() && temps >= m.regard_jusqua {
                (m.regard, m.prochain_regard) = (None, temps + 3.0 + 6.0 * m.hasard());
            } else if m.regard.is_none() && temps >= m.prochain_regard {
                let k = ((m.hasard() * regards.len() as f64) as usize).min(regards.len() - 1);
                (m.regard, m.regard_jusqua) = (Some(k), temps + 0.7 + 1.8 * m.hasard());
            }
        } else {
            m.regard = None;
        }
        pose.yeux = match yeux_pour(fermeture((temps - m.clin) / DUREE_DU_CLIN), yeux.len()) {
            Some(k) => Some(yeux[k].id()),
            None => m.regard.map(|k| regards[k].id()),
        };
        // Le souffle : une inspiration toutes les quatre secondes et demie (elle ne fait que grandir : le cadre reste dans
        // l'image). La tête : au repos, de petits mouvements lents (des ondes dont les périodes ne se répètent pas) — elle
        // penche de deux degrés au plus, se déplace d'un ou deux points ; en parlant, presque deux fois plus, et elle
        // hoche un peu la tête avec sa voix.
        if vivant {
            pose.souffle = 0.003 * (1.0 + (temps * std::f64::consts::TAU / 4.5).sin()) as f32;
            m.elan = suivre(m.elan, m.ouverture, 0.25);
            m.animation = suivre(m.animation, if parle { 1.0 } else { 0.0 }, 0.8);
            let onde = |periode: f64, phase: f64| (temps * std::f64::consts::TAU / periode + phase).sin() as f32;
            let amusee = if m.emotion == Some(Emotion::Amusee) { m.emotion_part } else { 0.0 };
            let ample = (1.0 + 0.8 * m.animation) * (1.0 + 0.5 * amusee);
            // Le penchant de l'émotion : la tête penchée par empathie, basse de tristesse, levée de surprise, de côté en
            // réfléchissant ; il vient et s'en va en une demi-seconde.
            let (roulis, x, y) = match m.emotion {
                Some(Emotion::Empathie) => (0.06, 0.01, 0.01),
                Some(Emotion::Triste) => (-0.02, 0.0, 0.035),
                Some(Emotion::Surprise) => (0.0, 0.0, -0.02),
                Some(Emotion::Reflexion) => (-0.035, -0.01, -0.01),
                _ => (0.0, 0.0, 0.0),
            };
            let part = m.emotion_part;
            m.biais = Tete {
                roulis: suivre(m.biais.roulis, roulis * part, 0.5),
                x: suivre(m.biais.x, x * part, 0.5),
                y: suivre(m.biais.y, y * part, 0.5),
            };
            pose.tete = Tete {
                roulis: m.biais.roulis + ample * (0.022 * onde(9.7, 0.0) + 0.012 * onde(4.3, 1.3)),
                x: m.biais.x + ample * (0.012 * onde(11.3, 2.1) + 0.006 * onde(3.1, 0.4)),
                y: m.biais.y + 0.006 * onde(6.1, 0.9) + 0.02 * m.elan,
            };
        } else if !parle && m.ouverture < 0.01 && emotion_posee {
            pose.apres = (m.clin - temps).max(0.0);
        }
        Some(pose)
    }

    /// Le dossier local, où vit le portrait.
    pub(crate) fn local(&self) -> &Path {
        &self.local
    }

    /// Retire le portrait : l'orbe revient tout de suite, sans attendre le prochain regard sur les fichiers.
    pub(crate) fn retirer(&mut self) {
        retirer(&self.local);
        (self.vu, self.visage) = (Vec::new(), None);
    }
}

/// La durée d'un clignement, en secondes.
const DUREE_DU_CLIN: f64 = 0.24;

/// La fermeture des yeux (0 : ouverts, 1 : fermés) à `p` (de 0 à 1) d'un clignement : ils se ferment vite, restent
/// fermés un instant, se rouvrent plus lentement.
fn fermeture(p: f64) -> f64 {
    match p {
        p if !(0.0..1.0).contains(&p) => 0.0,
        p if p < 0.3 => p / 0.3,
        p if p < 0.45 => 1.0,
        p => 1.0 - (p - 0.45) / 0.55,
    }
}

/// Le calque des yeux pour une fermeture (`n` calques, de plus en plus fermés) ; `None` : ouverts.
fn yeux_pour(fermeture: f64, n: usize) -> Option<usize> {
    let k = (fermeture * n as f64).round() as usize;
    (n > 0 && k > 0).then(|| k.min(n) - 1)
}

/// Les poses de la bouche pour une ouverture (de 0 à 1), parmi `n` de plus en plus ouvertes : celle d'en dessous (0 :
/// la photo) et la part de la suivante qui se fond en elle — sous un souffle, la photo ; l'ouverture courbée (à la
/// puissance 1,5) : les plus ouvertes, pour les syllabes les plus fortes seulement.
fn ouverture_en_poses(ouverture: f32, n: usize) -> (usize, f32) {
    const SEUIL: f32 = 0.15;
    let o = ouverture.clamp(0.0, 1.0).powf(1.5);
    let x = ((o - SEUIL) / (1.0 - SEUIL)).clamp(0.0, 1.0) * n as f32;
    let k = (x.floor() as usize).min(n);
    (k, if k < n { x - k as f32 } else { 0.0 })
}

/// Lit le portrait du dossier local, dans un fil : la photo, sa mise en place, et les calques de ses poses de sa taille
/// (une autre est laissée, et le journal le dit).
fn lire(local: &Path) -> Result<Images, String> {
    let chemin = trouver(local, "portrait").ok_or("le portrait a disparu")?;
    let photo = crate::cartes::image_du_disque(&chemin, COTE)?;
    let (detoure, cadre) = mise_en_place(&photo);
    let pose = |c: PathBuf| match crate::cartes::image_du_disque(&c, COTE) {
        Ok(i) if i.size == photo.size => Some(calque(&photo, &i)),
        Ok(_) => {
            crate::journal::ecrire(&format!("portrait d'Eve : {} n'a pas la taille de la photo", c.display()));
            None
        }
        Err(e) => {
            crate::journal::ecrire(&format!("portrait d'Eve : {} ne se lit pas : {e}", c.display()));
            None
        }
    };
    let calques = SORTES.map(|sorte| poses(local, "portrait", sorte).into_iter().map_while(pose).collect());
    let emotions = emotions(local, "portrait").into_iter().filter_map(|(e, c)| Some((e, pose(c)?))).collect();
    Ok(Images { photo, calques, emotions, detoure, cadre })
}

/// Les poses d'émotion posées dans `dossier` à côté de l'image `base` (`base-emotion-triste`…), une par émotion au
/// plus, sauf la neutre (la photo elle-même).
fn emotions(dossier: &Path, base: &str) -> Vec<(Emotion, PathBuf)> {
    Emotion::TOUTES
        .iter()
        .filter(|&&e| e != Emotion::Neutre)
        .filter_map(|&e| Some((e, trouver(dossier, &format!("{base}-emotion-{}", e.nom()))?)))
        .collect()
}

/// Le calque d'une pose : ce qui y change par rapport à la photo — plus de 10 sur 255 d'écart —, élargi et adouci sur
/// un soixantième de l'image pour que son bord ne se voie pas, le reste transparent. Un écart isolé (le grain de la
/// photo) ne compte pas.
fn calque(photo: &egui::ColorImage, pose: &egui::ColorImage) -> egui::ColorImage {
    let [l, h] = photo.size;
    let ecart = |a: Color32, b: Color32| {
        let d = |x: u8, y: u8| x.abs_diff(y);
        d(a.r(), b.r()).max(d(a.g(), b.g())).max(d(a.b(), b.b())).max(d(a.a(), b.a()))
    };
    let mut masque: Vec<f32> =
        photo.pixels.iter().zip(&pose.pixels).map(|(&a, &b)| if ecart(a, b) > 10 { 1.0 } else { 0.0 }).collect();
    flou(&mut masque, l, h, 2);
    for v in &mut masque {
        *v = if *v > 0.35 { 1.0 } else { 0.0 };
    }
    let r = (l.max(h) / 60).max(2);
    flou(&mut masque, l, h, r);
    for v in &mut masque {
        *v = (*v * 4.0).min(1.0);
    }
    flou(&mut masque, l, h, r);
    let pixels = pose
        .pixels
        .iter()
        .zip(&masque)
        .map(|(c, &a)| {
            let [r, g, b, al] = c.to_array().map(|v| (f32::from(v) * a).round() as u8);
            Color32::from_rgba_premultiplied(r, g, b, al)
        })
        .collect();
    egui::ColorImage::new([l, h], pixels)
}

/// Un flou en boîte de rayon `r` sur une image d'un seul canal (`l` sur `h`), sur place : les rangées, puis les
/// colonnes, chacune par ses sommes cumulées.
fn flou(m: &mut [f32], l: usize, h: usize, r: usize) {
    let mut ligne = Vec::new();
    let mut passe = |m: &mut [f32], n: usize, pas: usize, debut: usize| {
        ligne.clear();
        ligne.push(0.0f32);
        for i in 0..n {
            let s = ligne[i] + m[debut + i * pas];
            ligne.push(s);
        }
        for i in 0..n {
            let (a, b) = (i.saturating_sub(r), (i + r + 1).min(n));
            m[debut + i * pas] = (ligne[b] - ligne[a]) / (b - a) as f32;
        }
    };
    for y in 0..h {
        passe(m, l, 1, y * l);
    }
    for x in 0..l {
        passe(m, h, l, x);
    }
}

impl Visage {
    /// Le portrait en grand (la bannière, la tête de la conversation seule), dans le disque de centre `centre` et de
    /// rayon `rayon`, dans sa pose du moment ([`Portrait::pose_du_moment`]) : la photo, puis le calque de la bouche, puis
    /// celui des yeux — une photo, en disque ; détouré, sur son fond de studio, la personne que le bas du cercle coupe et
    /// dont la tête sort par le haut, et qui bouge, ses épaules restant en place ([`grille`]). Le souffle la grandit un
    /// peu depuis le bas du disque, comme une poitrine qui s'emplit.
    pub(crate) fn en_grand(&self, p: &egui::Painter, centre: Pos2, rayon: f32, pose: &Pose) {
        let k = 1.0 + pose.souffle;
        let cadre = Rect::from_center_size(
            pos2(self.cadre.center().x, self.cadre.max.y - self.cadre.height() / (2.0 * k)),
            self.cadre.size() / k,
        );
        let images = std::iter::once((self.texture.id(), 1.0))
            .chain(pose.emotion)
            .chain(pose.bouche.into_iter().flatten())
            .chain(pose.sourcils.into_iter().flatten())
            .chain(pose.yeux.map(|y| (y, 1.0)))
            .map(|(image, part)| (image, Color32::from_white_alpha((part.clamp(0.0, 1.0) * 255.0).round() as u8)));
        let Some(fond) = &self.fond else {
            for (image, teinte) in images {
                disque(p, centre, rayon, image, cadre, teinte);
            }
            return;
        };
        disque(p, centre, rayon, fond.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        let carre = Rect::from_center_size(centre, vec2(2.0 * rayon, 2.0 * rayon));
        // Un point de l'écran, et sa coordonnée de texture : le carré du disque est le cadre de l'image.
        let vers_uv = move |q: Pos2| {
            pos2(
                cadre.min.x + (q.x - carre.min.x) / carre.width() * cadre.width(),
                cadre.min.y + (q.y - carre.min.y) / carre.height() * cadre.height(),
            )
        };
        // Le haut de l'image, à l'écran : la tête s'arrête là. Le bas du disque, qui ne bouge pas ; puis le haut, en
        // grille, que la tête déforme.
        let haut = carre.min.y - cadre.min.y / cadre.height() * carre.height();
        for (image, teinte) in images {
            medaillon(p, centre, rayon, centre.y, (image, teinte), vers_uv);
            grille(p, centre, rayon, haut, (image, teinte), vers_uv, pose.tete);
        }
    }

    /// Chaque pose, nommée, pour l'essai du portrait : la photo, chaque calque seul (de chaque sorte, son nom et son
    /// rang), deux fondus (une bouche à moitié fondue dans la suivante ; les lèvres à moitié closes), et deux mélanges —
    /// cligner en parlant, regarder de côté en parlant.
    pub(crate) fn poses_d_essai(&self) -> Vec<(String, Pose)> {
        let [yeux, regards, ouvertes, ..] = &self.calques;
        let pose = |bouche: &[(&egui::TextureHandle, f32)], yeux: Option<&egui::TextureHandle>| {
            let mut p = Pose { yeux: yeux.map(egui::TextureHandle::id), ..Pose::default() };
            for (place, (t, part)) in p.bouche.iter_mut().zip(bouche) {
                *place = Some((t.id(), *part));
            }
            p
        };
        let mut v = vec![("la photo".to_string(), Pose::default())];
        for (sorte, calques) in SORTES.iter().zip(&self.calques) {
            for (k, t) in calques.iter().enumerate() {
                let p = match *sorte {
                    "yeux" | "regard" => pose(&[], Some(t)),
                    "sourcils" => Pose { sourcils: [Some((t.id(), 1.0)), None], ..Pose::default() },
                    _ => pose(&[(t, 1.0)], None),
                };
                v.push((format!("{sorte} {}", k + 1), p));
            }
        }
        if let [a, b, ..] = ouvertes.as_slice() {
            v.push(("bouche 1 et 2, en fondu".into(), pose(&[(a, 1.0), (b, 0.5)], None)));
        }
        if let (Some(s), Some(y)) = (self.calques[6].last(), yeux.last()) {
            let p = Pose { sourcils: [Some((s.id(), 1.0)), None], ..pose(&[], Some(y)) };
            v.push(("sourcils levés, en clignant".into(), p));
        }
        if let (Some(b), Some(f)) = (ouvertes.get(2), self.calques[5].first()) {
            v.push(("lèvres à demi closes".into(), pose(&[(b, 1.0), (f, 0.5)], None)));
        }
        if let (Some(b), Some(y)) = (ouvertes.get(ouvertes.len() / 2), yeux.last()) {
            v.push(("cligne en parlant".into(), pose(&[(b, 1.0)], Some(y))));
        }
        if let (Some(b), Some(r)) = (ouvertes.last(), regards.first()) {
            v.push(("regarde en parlant".into(), pose(&[(b, 1.0)], Some(r))));
        }
        for (e, t) in &self.emotions {
            v.push((format!("émotion : {}", e.nom()), Pose { emotion: Some((t.id(), 1.0)), ..Pose::default() }));
        }
        // La tête penchée, plus que jamais en vrai (le cou et la jonction avec les épaules s'y jugent), et en parlant.
        let penchee = Tete { roulis: 0.08, x: 0.03, y: 0.02 };
        v.push(("tête penchée".into(), Pose { tete: penchee, ..Pose::default() }));
        let autre = Tete { roulis: -0.08, x: -0.03, y: -0.01 };
        let bouche: Vec<_> = ouvertes.last().map(|b| (b, 1.0)).into_iter().collect();
        v.push(("penchée, en parlant".into(), Pose { tete: autre, ..pose(&bouche, None) }));
        v
    }

    /// Le petit disque (devant les bulles d'Eve, dans l'en-tête du dock et les réglages) : le visage, de plus près ;
    /// détouré, sur son fond de studio.
    pub(crate) fn vignette(&self, p: &egui::Painter, centre: Pos2, rayon: f32) {
        if let Some(fond) = &self.fond {
            disque(p, centre, rayon, fond.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        disque(p, centre, rayon, self.texture.id(), de_pres(self.cadre), Color32::WHITE);
    }
}

/// Une image du portrait dans `dossier` (`nom` : « portrait », « portrait-yeux-1 »…), sous la première de ses
/// [`EXTENSIONS`] qui existe.
fn trouver(dossier: &Path, nom: &str) -> Option<PathBuf> {
    EXTENSIONS.iter().map(|e| dossier.join(format!("{nom}.{e}"))).find(|c| c.is_file())
}

/// Les poses d'une [sorte](SORTES) posées dans `dossier` à côté de l'image `base` : `base-yeux-1`, `base-yeux-2`…,
/// jusqu'à la première qui manque.
fn poses(dossier: &Path, base: &str, sorte: &str) -> Vec<PathBuf> {
    (1..=POSES_MAX).map_while(|k| trouver(dossier, &format!("{base}-{sorte}-{k}"))).collect()
}

/// Les fichiers du portrait dans le dossier local : la photo, puis ses poses ; rien sans photo.
fn fichiers(local: &Path) -> Vec<PathBuf> {
    let Some(photo) = trouver(local, "portrait") else { return Vec::new() };
    std::iter::once(photo)
        .chain(SORTES.iter().flat_map(|s| poses(local, "portrait", s)))
        .chain(emotions(local, "portrait").into_iter().map(|(_, c)| c))
        .collect()
}

/// Pose `choisie` comme portrait d'Eve : une image que Windows sait lire, copiée dans le dossier local sous son nom de
/// portrait (l'image d'origine peut ensuite bouger), avec les poses posées à côté d'elle sous son nom (« eve.png »,
/// « eve-yeux-1.png », « eve-bouche-1.png »…) ; le portrait d'avant et ses poses s'en vont.
pub(crate) fn poser(choisie: &Path, local: &Path) -> Result<PathBuf, String> {
    crate::cartes::image_du_disque(choisie, 64)
        .map_err(|_| "ce n'est pas une image que Windows sait lire".to_string())?;
    let dossier = choisie.parent().unwrap_or(Path::new("."));
    if dossier.canonicalize().ok() == local.canonicalize().ok() {
        return Err("cette image est déjà dans le dossier d'Eve : choisis-la ailleurs".into());
    }
    let base = choisie.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    retirer(local);
    let vers = copier(choisie, local, "portrait")?;
    let mut n = 0;
    for sorte in SORTES {
        for (k, c) in poses(dossier, &base, sorte).iter().enumerate() {
            copier(c, local, &format!("portrait-{sorte}-{}", k + 1))?;
            n += 1;
        }
    }
    for (e, c) in emotions(dossier, &base) {
        copier(&c, local, &format!("portrait-emotion-{}", e.nom()))?;
        n += 1;
    }
    let quoi = format!("portrait d'Eve posé : {} (de {}), {n} pose(s)", vers.display(), choisie.display());
    crate::journal::ecrire(&quoi);
    Ok(vers)
}

/// Copie une image du portrait dans le dossier local sous `nom`, son extension gardée.
fn copier(de: &Path, local: &Path, nom: &str) -> Result<PathBuf, String> {
    let ext = de.extension().map(|e| e.to_string_lossy().to_lowercase());
    let ext = ext.filter(|e| EXTENSIONS.contains(&e.as_str())).unwrap_or_else(|| "png".into());
    let vers = local.join(format!("{nom}.{ext}"));
    std::fs::copy(de, &vers).map_err(|e| e.to_string())?;
    Ok(vers)
}

/// Retire le portrait d'Eve du dossier local, et ses poses, sous tous leurs noms.
fn retirer(local: &Path) {
    let noms = SORTES.iter().flat_map(|s| (1..=POSES_MAX).map(move |k| format!("portrait-{s}-{k}")));
    let des_emotions = Emotion::TOUTES.iter().map(|e| format!("portrait-emotion-{}", e.nom()));
    for nom in std::iter::once("portrait".to_string()).chain(noms).chain(des_emotions) {
        for e in EXTENSIONS {
            let _ = std::fs::remove_file(local.join(format!("{nom}.{e}")));
        }
    }
}

/// La part d'une photo qui se montre dans le disque : un carré, au centre d'une image plus large ; dans une image plus
/// haute, plus près du haut (le visage d'un portrait).
fn carre_de_l_image(largeur: f32, hauteur: f32) -> Rect {
    if largeur >= hauteur {
        let m = (1.0 - hauteur / largeur) / 2.0;
        Rect::from_min_max(pos2(m, 0.0), pos2(1.0 - m, 1.0))
    } else {
        let part = largeur / hauteur;
        let haut = (1.0 - part) * 0.2;
        Rect::from_min_max(pos2(0.0, haut), pos2(1.0, haut + part))
    }
}

/// La mise en place d'une image : détourée (plus d'un quart de son bord transparent), et le carré du disque de la grande
/// vue. Une photo : son carré ([`carre_de_l_image`]). Un portrait détouré : le cercle d'après la personne (ses pixels
/// opaques) — sous le milieu de sa tête, le haut de sa tête à [`DEPASSE`] du diamètre au-dessus de lui, son bas dans
/// l'image, pour que le cercle coupe le buste, pas le bord de l'image.
fn mise_en_place(image: &egui::ColorImage) -> (bool, Rect) {
    let [l, h] = image.size;
    let photo = (false, carre_de_l_image(l as f32, h as f32));
    if l < 8 || h < 8 {
        return photo;
    }
    let opaque = |x: usize, y: usize| image.pixels[y * l + x].a() >= 128;
    let bord = (0..l).flat_map(|x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(|y| [(0, y), (l - 1, y)]));
    let (n, transparents) = bord.fold((0, 0), |(n, t), (x, y)| (n + 1, t + usize::from(!opaque(x, y))));
    if transparents * 4 < n {
        return photo;
    }
    let rangees: Vec<usize> = (0..h).filter(|&y| (0..l).any(|x| opaque(x, y))).collect();
    let colonnes: Vec<usize> = (0..l).filter(|&x| (0..h).any(|y| opaque(x, y))).collect();
    let (Some(&haut), Some(&bas), Some(&gauche), Some(&droite)) =
        (rangees.first(), rangees.last(), colonnes.first(), colonnes.last())
    else {
        return photo;
    };
    // Le milieu de la tête : celui des pixels opaques du haut de la personne.
    let tete = haut + (bas - haut) * 12 / 100;
    let (somme, nombre) = (haut..=tete)
        .flat_map(|y| (0..l).map(move |x| (x, y)))
        .filter(|&(x, y)| opaque(x, y))
        .fold((0, 0), |(s, n), (x, _)| (s + x, n + 1));
    let milieu = if nombre > 0 { somme as f32 / nombre as f32 } else { (gauche + droite) as f32 / 2.0 };
    // Le diamètre : le bas du cercle un peu au-dessus du bas de la personne, pas plus large qu'elle ni que l'image.
    let d = ((bas - haut) as f32 / (1.0 + DEPASSE + 0.02)).min((droite - gauche + 1) as f32).min(l as f32).max(1.0);
    let x = (milieu - d / 2.0).clamp(0.0, l as f32 - d);
    let y = haut as f32 + DEPASSE * d;
    (true, Rect::from_min_size(pos2(x / l as f32, y / h as f32), vec2(d / l as f32, d / h as f32)))
}

/// Le carré du visage, de plus près : celui du disque de la grande vue, [`DE_PRES`] fois plus petit, centré à
/// [`HAUTEUR_DU_VISAGE`] de son haut, déplacé au besoin pour rester dans l'image.
fn de_pres(cadre: Rect) -> Rect {
    let r = Rect::from_center_size(
        pos2(cadre.center().x, cadre.min.y + HAUTEUR_DU_VISAGE * cadre.height()),
        cadre.size() / DE_PRES,
    );
    let dx = (-r.min.x).max(0.0) - (r.max.x - 1.0).max(0.0);
    let dy = (-r.min.y).max(0.0) - (r.max.y - 1.0).max(0.0);
    r.translate(vec2(dx, dy))
}

/// Le fond du disque d'un portrait détouré : la lumière d'un studio, claire derrière la tête, qui descend au bleu pâle de
/// l'ERP sur les bords.
fn fond_de_studio(ctx: &egui::Context) -> egui::TextureHandle {
    const N: usize = 64;
    let clair = Color32::from_rgb(0xF7, 0xFA, 0xFF);
    let pixels = (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32 / (N - 1) as f32, (i / N) as f32 / (N - 1) as f32);
            super::melange(clair, erp::FOND_1, (vec2(x - 0.5, y - 0.38).length() / 0.7).clamp(0.0, 1.0))
        })
        .collect();
    ctx.load_texture("fond-du-portrait", egui::ColorImage::new([N, N], pixels), egui::TextureOptions::LINEAR)
}

/// Une texture peinte en disque, centré sur `centre`, de rayon `rayon` : sa part `uv` (son bord adouci par egui),
/// multipliée par `teinte` (blanc : telle quelle ; un blanc à moitié transparent : à moitié fondue).
fn disque(p: &egui::Painter, centre: Pos2, rayon: f32, texture: egui::TextureId, uv: Rect, teinte: Color32) {
    let carre = Rect::from_center_size(centre, vec2(2.0 * rayon, 2.0 * rayon));
    p.add(egui::epaint::RectShape::filled(carre, rayon, teinte).with_texture(texture, uv));
}

/// La personne détourée, en grand : un seul maillage texturé — le carré du disque, du haut de l'image (`haut`, à
/// l'écran) jusqu'au milieu du cercle, d'où la tête peut sortir, puis la moitié basse du cercle —, son contour adouci
/// sur un point, d'une frange qui s'efface (egui n'adoucit pas un maillage) ; `image` : sa texture et sa teinte (comme
/// [`disque`]) ; `vers_uv` : la coordonnée de texture d'un point de l'écran.
fn medaillon(
    p: &egui::Painter,
    centre: Pos2,
    rayon: f32,
    haut: f32,
    (texture, teinte): (egui::TextureId, Color32),
    vers_uv: impl Fn(Pos2) -> Pos2,
) {
    const N: usize = 64;
    // Le contour, dans le sens des aiguilles d'une montre à l'écran : les deux coins du haut, puis la moitié basse du
    // cercle, de droite à gauche.
    let mut contour = vec![pos2(centre.x - rayon, haut), pos2(centre.x + rayon, haut)];
    contour.extend(
        (0..=N).map(|k| centre + vec2((PI * k as f32 / N as f32).cos(), (PI * k as f32 / N as f32).sin()) * rayon),
    );
    let k = contour.len();
    let mut m = egui::Mesh::with_texture(texture);
    let milieu = pos2(centre.x, (haut + centre.y + rayon) / 2.0);
    m.vertices.push(egui::epaint::Vertex { pos: milieu, uv: vers_uv(milieu), color: teinte });
    for (i, &point) in contour.iter().enumerate() {
        // Dehors : la perpendiculaire au contour, à gauche de sa marche (l'écran a son y vers le bas).
        let marche = (contour[(i + 1) % k] - contour[(i + k - 1) % k]).normalized();
        let dehors = point + vec2(marche.y, -marche.x);
        for (q, couleur) in [(point, teinte), (dehors, Color32::TRANSPARENT)] {
            m.vertices.push(egui::epaint::Vertex { pos: q, uv: vers_uv(q), color: couleur });
        }
    }
    for i in 0..k as u32 {
        let j = (i + 1) % k as u32;
        let (dedans, frange, dedans_j, frange_j) = (1 + 2 * i, 2 + 2 * i, 1 + 2 * j, 2 + 2 * j);
        m.indices.extend([0, dedans, dedans_j, dedans, frange, dedans_j, frange, frange_j, dedans_j]);
    }
    p.add(egui::Shape::mesh(m));
}

/// Le haut du médaillon — du haut de l'image (`haut`, à l'écran) au milieu du disque —, en grille, chaque point déplacé
/// par la `tete` : penché autour de la base du cou et déplacé, pleinement au-dessus du menton, de moins en moins le long
/// du cou, plus du tout au milieu du disque, où la grille rejoint le bas, qui ne bouge pas ; la texture prise là où le
/// point était (le visage suit la tête). `image` et `vers_uv` : comme [`medaillon`].
fn grille(
    p: &egui::Painter,
    centre: Pos2,
    rayon: f32,
    haut: f32,
    (texture, teinte): (egui::TextureId, Color32),
    vers_uv: impl Fn(Pos2) -> Pos2,
    tete: Tete,
) {
    const COLONNES: u32 = 16;
    const RANGEES: u32 = 20;
    let pivot = centre + vec2(0.0, 0.05 * rayon);
    let (s, c) = tete.roulis.sin_cos();
    let deplace = |q: Pos2| {
        let t = ((centre.y - q.y) / (0.15 * rayon)).clamp(0.0, 1.0);
        let poids = t * t * (3.0 - 2.0 * t);
        let d = q - pivot;
        let penche = vec2(d.x * c - d.y * s, d.x * s + d.y * c);
        q + (penche - d + vec2(tete.x, tete.y) * rayon) * poids
    };
    let mut m = egui::Mesh::with_texture(texture);
    for j in 0..=RANGEES {
        let y = haut + (centre.y - haut) * j as f32 / RANGEES as f32;
        for i in 0..=COLONNES {
            let q = pos2(centre.x - rayon + 2.0 * rayon * i as f32 / COLONNES as f32, y);
            m.vertices.push(egui::epaint::Vertex { pos: deplace(q), uv: vers_uv(q), color: teinte });
        }
    }
    let largeur = COLONNES + 1;
    for j in 0..RANGEES {
        for i in 0..COLONNES {
            let a = j * largeur + i;
            m.indices.extend([a, a + 1, a + largeur, a + 1, a + largeur + 1, a + largeur]);
        }
    }
    p.add(egui::Shape::mesh(m));
}

/// L'anneau d'état autour du portrait (`centre`, `rayon` : ceux du disque), ce que fait Eve comme l'orbe le montrait,
/// dans `couleur` (celle de son état) : prête, un filet clair ; elle écoute, un anneau qui bat ; elle transcrit,
/// réfléchit ou se prépare, un arc qui tourne ; elle parle, des ondes qui suivent sa voix (`niveau`) ; un accord, la
/// couleur qui respire ; une panne, l'anneau plein. Peint avant le portrait : la tête qui sort du cercle passe devant.
pub(crate) fn anneau(
    p: &egui::Painter,
    centre: Pos2,
    rayon: f32,
    etat: &str,
    couleur: Color32,
    niveau: f32,
    temps: f64,
) {
    let t = temps as f32;
    let epais = (rayon * 0.045).clamp(2.0, 5.0);
    let r = rayon + epais * 1.6;
    match etat {
        "ecoute" => {
            for k in 0..2 {
                let f = (t * 1.3 + k as f32 * 0.5) % 1.0;
                p.circle_stroke(centre, r + f * rayon * 0.22, Stroke::new(epais, couleur.gamma_multiply(1.0 - f)));
            }
            p.circle_stroke(centre, r, Stroke::new(epais, couleur));
        }
        "transcription" | "reflexion" | "demarrage" => {
            p.circle_stroke(centre, r, Stroke::new(epais, couleur.gamma_multiply(0.3)));
            let vitesse = if etat == "demarrage" { 1.5 } else { 3.0 };
            arc_qui_tourne(p, centre, r, epais, couleur, t * vitesse);
        }
        "parole" => {
            let onde = (t * 7.0).sin().abs() * niveau.max(0.4);
            p.circle_stroke(centre, r, Stroke::new(epais * (1.0 + onde), couleur));
            let f = (t * 0.9) % 1.0;
            let large = Stroke::new(epais * 0.6, couleur.gamma_multiply(0.6 * (1.0 - f)));
            p.circle_stroke(centre, r + f * rayon * 0.2, large);
        }
        "confirmation" => {
            let souffle = 0.55 + 0.45 * (t * 2.2).sin();
            p.circle_stroke(centre, r, Stroke::new(epais * 1.3, couleur.gamma_multiply(souffle)));
        }
        "erreur" => {
            p.circle_stroke(centre, r, Stroke::new(epais, couleur));
        }
        _ => {
            p.circle_stroke(centre, r - epais * 0.5, Stroke::new(epais * 0.6, Color32::from_white_alpha(170)));
        }
    }
}

/// La pastille « en ligne » d'Eve prête, en bas à droite du disque, par-dessus le portrait.
pub(crate) fn pastille(p: &egui::Painter, centre: Pos2, rayon: f32, etat: &str, couleur: Color32) {
    if etat == "pret" {
        let ici = centre + vec2(FRAC_PI_4.cos(), FRAC_PI_4.sin()) * rayon * 0.92;
        let d = (rayon * 0.15).clamp(4.0, 12.0);
        p.circle_filled(ici, d + 2.5, Color32::WHITE);
        p.circle_filled(ici, d, couleur);
    }
}

/// Un arc lumineux qui tourne sur le cercle de rayon `r` (`angle` : sa tête) : un seul trait, sa lueur dessous, sa
/// traîne qui s'efface sur [`TRAINE`] — la couleur de chaque point suit son angle, sans les coutures de petits traits.
fn arc_qui_tourne(p: &egui::Painter, centre: Pos2, r: f32, epais: f32, couleur: Color32, angle: f32) {
    let n = 48;
    let points: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = angle - TRAINE * k as f32 / n as f32;
            centre + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let trait_ = |largeur: f32, part: f32| {
        egui::epaint::PathStroke::new_uv(largeur, move |_, point| {
            // Ce qui dépasse un peu la tête (le bout du trait, son adoucissement) compte comme la tête.
            let derriere = (angle - (point - centre).angle()).rem_euclid(TAU);
            let derriere = if derriere > TAU - 0.3 { 0.0 } else { derriere };
            couleur.gamma_multiply(part * (1.0 - derriere / TRAINE).clamp(0.0, 1.0))
        })
    };
    p.add(egui::Shape::line(points.clone(), trait_(epais * 3.2, 0.22)));
    p.add(egui::Shape::line(points, trait_(epais * 1.25, 1.0)));
}

/// La longueur de la traîne de l'arc qui tourne, en radians.
const TRAINE: f32 = 2.4;

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_visage_reste_dans_le_disque() {
        // Une image plus large : le carré du milieu ; plus haute : un carré près du haut, où est le visage.
        let large = carre_de_l_image(1600.0, 900.0);
        assert!((large.width() - 900.0 / 1600.0).abs() < 1e-6 && (large.center().x - 0.5).abs() < 1e-6);
        let haute = carre_de_l_image(800.0, 1200.0);
        assert!((haute.height() - 800.0 / 1200.0).abs() < 1e-6);
        assert!(haute.top() > 0.0 && haute.top() < (1.0 - haute.height()) / 2.0, "plus près du haut : {haute:?}");
        assert_eq!(carre_de_l_image(500.0, 500.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        // De plus près : plus petit, toujours dans l'image, même pour un carré qui touche son bord.
        for cadre in [large, haute, carre_de_l_image(500.0, 500.0)] {
            let r = de_pres(cadre);
            assert!((r.width() - cadre.width() / DE_PRES).abs() < 1e-6, "{r:?}");
            assert!(r.min.x >= -1e-6 && r.min.y >= -1e-6 && r.max.x <= 1.0 + 1e-6 && r.max.y <= 1.0 + 1e-6, "{r:?}");
        }
        // Le dossier sans portrait : rien ; un portrait et ses poses — la deuxième bouche manque : la troisième ne compte
        // pas —, puis retirés, tous.
        let local = std::env::temp_dir().join(format!("eve-essai-portrait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        std::fs::create_dir_all(&local).unwrap();
        assert!(fichiers(&local).is_empty());
        for nom in ["portrait-yeux-1.png", "portrait.jpg", "portrait-yeux-2.png", "portrait-bouche-1.png"] {
            std::fs::write(local.join(nom), b"x").unwrap();
        }
        std::fs::write(local.join("portrait-bouche-3.png"), b"x").unwrap();
        let noms: Vec<String> =
            fichiers(&local).iter().map(|c| c.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(noms, ["portrait.jpg", "portrait-yeux-1.png", "portrait-yeux-2.png", "portrait-bouche-1.png"]);
        // Une image choisie ailleurs, et ses poses à côté d'elle, sous son nom : copiées sous ceux du portrait.
        let ailleurs = local.join("ailleurs");
        std::fs::create_dir_all(&ailleurs).unwrap();
        std::fs::write(ailleurs.join("eve-bouche-1.webp"), b"x").unwrap();
        assert_eq!(poses(&ailleurs, "eve", "bouche"), [ailleurs.join("eve-bouche-1.webp")]);
        assert!(poses(&ailleurs, "eve", "yeux").is_empty());
        let copie = copier(&ailleurs.join("eve-bouche-1.webp"), &local, "portrait-bouche-2").unwrap();
        assert_eq!(copie, local.join("portrait-bouche-2.webp"));
        assert_eq!(fichiers(&local).len(), 6);
        retirer(&local);
        assert!(fichiers(&local).is_empty());
        assert!(
            std::fs::read_dir(&local).unwrap().all(|e| e.unwrap().path() == ailleurs),
            "tout le portrait est parti"
        );
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn elle_cligne_et_sa_bouche_suit_sa_voix() {
        // Le clignement : les yeux se ferment vite, restent fermés, se rouvrent plus lentement ; ouverts hors de lui.
        let f = [-0.1, 0.0, 0.15, 0.3, 0.4, 0.725, 1.0].map(fermeture);
        let attendu = [0.0, 0.0, 0.5, 1.0, 1.0, 0.5, 0.0];
        assert!(f.iter().zip(attendu).all(|(a, b)| (a - b).abs() < 1e-9), "{f:?}");
        // Trois calques des yeux : de plus en plus fermés ; ouverts, aucun.
        let k: Vec<_> = [0.0, 0.1, 0.2, 0.5, 0.9, 1.0].map(|f| yeux_pour(f, 3)).into();
        assert_eq!(k, [None, None, Some(0), Some(1), Some(2), Some(2)]);
        assert_eq!(yeux_pour(1.0, 0), None);
        // Trois bouches : la photo sous un souffle ; puis la pose d'en dessous et la part de la suivante qui s'y fond ; la
        // plus grande, pour les plus fortes.
        let poses = [0.0, 0.5, 0.8, 1.0].map(|o| ouverture_en_poses(o, 3));
        let attendu = [(0, 0.0), (0, 0.7185), (1, 0.996), (3, 0.0)];
        for ((k, part), (k2, part2)) in poses.into_iter().zip(attendu) {
            assert!(k == k2 && (part - part2).abs() < 0.01, "{poses:?}");
        }
        assert_eq!(ouverture_en_poses(1.0, 0), (0, 0.0));
    }

    #[test]
    fn un_calque_ne_garde_que_ce_qui_change() {
        // Une photo grise de 120 sur 120 ; une pose où un carré de 10 change (les yeux qui se ferment), et un pixel seul
        // (le grain de la photo).
        let gris = Color32::from_rgb(120, 120, 120);
        let photo = egui::ColorImage::new([120, 120], vec![gris; 120 * 120]);
        let mut pose = photo.clone();
        for y in 30..40 {
            for x in 50..60 {
                pose.pixels[y * 120 + x] = Color32::from_rgb(60, 40, 30);
            }
        }
        pose.pixels[100 * 120 + 10] = Color32::WHITE;
        let c = calque(&photo, &pose);
        let alpha = |x: usize, y: usize| c.pixels[y * 120 + x].a();
        assert_eq!(alpha(55, 35), 255, "le carré, entier");
        assert!(alpha(48, 35) > 0, "et un peu autour, adouci");
        assert_eq!(alpha(10, 100), 0, "le pixel seul ne compte pas");
        assert_eq!(alpha(100, 100), 0, "le reste, transparent");
        assert_eq!(c.pixels[35 * 120 + 55], Color32::from_rgb(60, 40, 30), "la couleur de la pose");
    }

    #[test]
    fn un_portrait_detoure_sort_du_cercle() {
        // Un buste inventé sur un fond transparent, 200 sur 160 : la tête (un disque de rayon 25 centré en 120, 40), les
        // épaules (de 70 jusqu'au bas de l'image, de 30 à 190 en largeur).
        let (l, h) = (200, 160);
        let pixels = (0..l * h)
            .map(|i| {
                let (x, y) = ((i % l) as f32, (i / l) as f32);
                let tete = vec2(x - 120.0, y - 40.0).length() <= 25.0;
                let epaules = y >= 70.0 && (30.0..=190.0).contains(&x);
                if tete || epaules { Color32::from_rgb(90, 60, 40) } else { Color32::TRANSPARENT }
            })
            .collect();
        let (detoure, cadre) = mise_en_place(&egui::ColorImage::new([l, h], pixels));
        assert!(detoure);
        let (haut, d) = (cadre.min.y * h as f32, cadre.width() * l as f32);
        // Le haut de la tête (15) au-dessus du cercle, de DEPASSE du diamètre ; le cercle carré, centré sous la tête,
        // son bas dans l'image.
        assert!((haut - (15.0 + DEPASSE * d)).abs() < 1.0, "{cadre:?}");
        assert!((cadre.height() * h as f32 - d).abs() < 1e-3, "{cadre:?}");
        assert!((cadre.center().x * l as f32 - 120.0).abs() < 2.0, "{cadre:?}");
        assert!(cadre.max.y < 1.0, "{cadre:?}");
        // Une image opaque : une photo, son carré du milieu.
        let photo = egui::ColorImage::new([l, h], vec![Color32::from_rgb(200, 200, 200); l * h]);
        assert_eq!(mise_en_place(&photo), (false, carre_de_l_image(l as f32, h as f32)));
    }
}
