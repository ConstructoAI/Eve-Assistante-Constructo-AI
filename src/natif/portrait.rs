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
//! serrées », « sa tête qui est toujours fixe », « des émotions triste, empathie, drôle… », « sa tête vague un peu…
//! l'expression faciale encore plus fluide »), si elles sont là, de la même taille que la photo : les yeux qui se ferment
//! (`portrait-yeux-1`, `-2`…), des regards de côté (`portrait-regard-1`…), les sourcils levés (`portrait-sourcils-1`…),
//! la bouche — ouverte, la mâchoire de plus en plus basse (`portrait-bouche-1`…), ronde (`portrait-rond-1`…), étirée
//! (`portrait-large-1`…), et les lèvres jointes : closes (`portrait-ferme-1`), closes et arrondies (`-ferme-2`), la
//! lèvre du bas sous les dents (`-ferme-3`) —, et une pose par émotion (`portrait-emotion-joie`…), avec ses bouches
//! pour parler (`portrait-emotion-joie-bouche-1`…) et ses lèvres closes (`portrait-emotion-joie-ferme`) ; et son visage
//! calme (`portrait-calme` : « elle est trop souriante quand elle parle »), le sourire adouci et les lèvres closes, le
//! bas du visage seul. Chacune devient un calque — ce qui y change par
//! rapport à la photo, le reste transparent, recadré sur ce qui s'y voit —, et les calques se posent l'un sur l'autre, en
//! fondu : en grand, elle cligne et regarde de côté de temps en temps, même en parlant ; sa bouche suit sa voix et la
//! forme de ses syllabes, un peu en avance sur le son, comme chez une vraie personne ([`crate::voix::bouche_a_venir`]),
//! et passe d'une forme à l'autre en fondu ; ses sourcils se lèvent sur une syllabe appuyée ; elle respire — sa
//! poitrine et ses épaules se soulèvent, plus vite avant de parler ; sa tête prend une posture et la garde, puis une
//! autre, et hoche sur une syllabe appuyée ; son visage prend l'émotion de sa réponse, que Claude choisit
//! ([`Emotion`]), et parle avec la bouche de cette émotion. Tout ce qui bouge suit un ressort : il part doucement et se
//! pose sans à-coup, comme un muscle.
//!
//! Ses parties, chacune dans son fichier : la mimique les assemble ([`mimique`]) — les émotions ([`emotions`]), la
//! bouche ([`bouche`]), les gestes de la tête et des sourcils ([`gestes`]), le buste et son souffle ([`corps`],
//! [`souffle`]), les cheveux qui suivent la tête avec un temps de retard ([`cheveux`]), le regard ([`regard`]) — et le
//! rendu les peint ([`rendu`]), sur un treillis que le flot de chaque pose déforme ([`flot`]), dans des textures à la
//! taille où il se montre ([`nettete`]). Ici : le portrait, ses fichiers, ses calques et ses poses.

mod bouche;
mod cheveux;
mod corps;
mod emotions;
mod flot;
mod gestes;
mod mimique;
mod nettete;
mod regard;
mod rendu;
mod souffle;

use std::f32::consts::{FRAC_PI_4, TAU};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2, vec2};
use eve::parole::Emotion;

/// Les extensions que peut porter le portrait, et chacune de ses poses.
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];
/// Les sortes de poses (`portrait-yeux-1`, `portrait-regard-1`, `portrait-bouche-1`…), douze au plus chacune : les yeux
/// qui se ferment, les regards ; puis la bouche — ouverte (a, è), ronde (o, ou, u), étirée (i, é), chacune de plus en
/// plus ouverte, et les lèvres jointes : closes (m, b, p), closes et arrondies (avant un o, un ou), la lèvre du bas
/// sous les dents (f, v) ; les sourcils, de plus en plus levés. Et, à part, une pose par émotion
/// (`portrait-emotion-triste`…), ses bouches (`portrait-emotion-triste-bouche-1`…) et ses lèvres closes
/// (`portrait-emotion-joie-ferme`).
const SORTES: [&str; 7] = ["yeux", "regard", "bouche", "rond", "large", "ferme", "sourcils"];
const POSES_MAX: usize = 12;
/// Le grand côté du portrait chargé, en pixels : de quoi la bannière d'un écran 4K.
const COTE: usize = 768;
/// Ce que la tête d'un portrait détouré dépasse du cercle, en grand : une part du diamètre.
const DEPASSE: f32 = 0.10;
/// Le visage de plus près, dans les petits disques : le grossissement, et la hauteur du visage dans le disque de la
/// grande vue (une part de son diamètre, depuis son haut).
const DE_PRES: f32 = 1.6;
const HAUTEUR_DU_VISAGE: f32 = 0.3;
/// Le bord transparent gardé autour d'un calque recadré, en pixels : les réductions de l'image que fait la carte
/// graphique (jusqu'au trente-deuxième de sa taille) n'y mêlent rien de la pose.
const MARGE: usize = 32;
/// Les textures du portrait : lissées, et réduites d'avance par la carte graphique (mipmaps) — sans elles, une image
/// montrée plus petite qu'elle n'est scintille dès qu'elle bouge d'une fraction de pixel (les mèches, quand la tête
/// bouge).
const LISSE: egui::TextureOptions =
    egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };

/// Un calque prêt à peindre : sa texture — la part de l'image où sa pose change, recadrée ([`recadrer`]) — et ses
/// pixels (gardés pour la refaire à la taille où elle se montre : [`nettete`]), le rectangle qu'elle couvre dans l'image
/// entière (en coordonnées de texture, de 0 à 1), et sa géométrie : où chaque point de la photo se retrouve dans sa pose
/// ([`rendu::Geometrie`]).
#[derive(Clone)]
struct Calque {
    texture: egui::TextureHandle,
    image: Arc<egui::ColorImage>,
    uv: Rect,
    geometrie: Arc<rendu::Geometrie>,
}

/// Une émotion du visage : sa pose (le visage qui la prend) et ses bouches pour parler, de plus en plus ouvertes (s'il
/// en a), chacune la bouche seule, à poser sur ce visage : elle parle sans quitter son émotion ; et ses lèvres closes
/// (m, b, p), la bouche seule aussi, si sa pose ne les a pas (la joie, la surprise).
#[derive(Clone)]
struct Expression {
    emotion: Emotion,
    visage: Calque,
    bouches: Vec<Calque>,
    levres_closes: Option<Calque>,
}

/// Le portrait prêt à peindre : sa photo (sa texture, ses pixels) et sa taille (en pixels) ; les calques de ses poses
/// ([`calque`]), une liste par [sorte](SORTES) — les yeux de plus en plus fermés, les regards, la bouche de plus en
/// plus ouverte… — et ses émotions ; son visage calme ([`bouche`]) ; détouré, la souplesse de ses cheveux, s'il a leur
/// masque ([`cheveux`]), et le fond de son disque ; le carré de l'image (en coordonnées de texture, de 0 à 1) que
/// couvre le disque de la grande vue ; et sa netteté ([`nettete`]) : la taille où il se montre en grand, les textures
/// refaites à cette taille, ses vignettes.
#[derive(Clone)]
pub(crate) struct Visage {
    texture: egui::TextureHandle,
    image: Arc<egui::ColorImage>,
    taille: [usize; 2],
    calques: [Vec<Calque>; 7],
    expressions: Vec<Expression>,
    calme: Option<Calque>,
    cheveux: Option<Arc<cheveux::Souplesse>>,
    fond: Option<egui::TextureHandle>,
    cadre: Rect,
    demande: Arc<nettete::Demande>,
    net: Option<Arc<nettete::Jeu>>,
    vignettes: Arc<nettete::Vignettes>,
}

/// Une pose lue par le fil de lecture : son calque recadré, le rectangle qu'il couvre dans l'image entière, et sa
/// géométrie.
struct Recadre {
    image: egui::ColorImage,
    uv: Rect,
    geometrie: rendu::Geometrie,
}

/// Ce que rend le fil de lecture : la photo et les calques de ses poses, ses émotions (leur visage, leurs bouches,
/// leurs lèvres closes), son visage calme, la souplesse de ses cheveux, détourée ou non, le carré de son disque.
struct Images {
    photo: egui::ColorImage,
    calques: [Vec<Recadre>; 7],
    expressions: Vec<(Emotion, Recadre, Vec<Recadre>, Option<Recadre>)>,
    calme: Option<Recadre>,
    cheveux: Option<cheveux::Souplesse>,
    detoure: bool,
    cadre: Rect,
}

/// Une couche de la pose : un calque (sa texture, le rectangle qu'il couvre dans l'image entière, sa géométrie — aucune
/// pour la photo) et sa part, de 0 à 1.
#[derive(Clone)]
struct Couche {
    texture: egui::TextureId,
    uv: Rect,
    part: f32,
    geometrie: Option<Arc<rendu::Geometrie>>,
}

impl Couche {
    /// Un point de l'image entière (en coordonnées de texture), dans celles du calque recadré : hors de lui, sa marge
    /// transparente.
    fn dans(&self, q: Pos2) -> Pos2 {
        pos2((q.x - self.uv.min.x) / self.uv.width(), (q.y - self.uv.min.y) / self.uv.height())
    }

    /// Sa teinte : un blanc, transparent à proportion de sa part (à moitié fondue : un blanc à moitié transparent).
    fn teinte(&self) -> Color32 {
        Color32::from_white_alpha((self.part.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

/// Les strates de la pose, de la plus basse à la plus haute : ce que l'émotion couvre (les sourcils levés, le regard de
/// côté) ; l'émotion ; la bouche ; ce qui passe par-dessus tout (le clignement). Chaque partie du visage pose ses calques
/// dans la sienne, dans l'ordre où elle les pose.
#[derive(Clone, Copy)]
enum Strate {
    Dessous,
    Emotion,
    Bouche,
    Dessus,
}

/// La pose du visage en grand à un instant ([`Portrait::pose_du_moment`]) : les calques à poser sur la photo, chacun
/// avec sa part, strate par strate ([`Strate`]) ; sa tête, son corps, le retard de ses cheveux sur sa tête, et dans
/// combien de secondes la redessiner.
#[derive(Clone, Default)]
pub(crate) struct Pose {
    couches: [Vec<Couche>; 4],
    tete: Tete,
    corps: Corps,
    cheveux: cheveux::Retard,
    pub(crate) apres: f64,
}

impl Pose {
    /// Pose un calque dans sa strate, à sa part (presque rien : aucun).
    fn peindre(&mut self, strate: Strate, c: &Calque, part: f32) {
        if part > 0.004 {
            let geometrie = Some(c.geometrie.clone());
            self.couches[strate as usize].push(Couche {
                texture: c.texture.id(),
                uv: c.uv,
                part: part.min(1.0),
                geometrie,
            });
        }
    }

    /// Une série de calques de plus en plus marqués, à l'avance `x` (de 0 à 1) : celui d'en dessous, et la part du
    /// suivant qui se fond en lui ([`en_poses`]) ; le tout à la part `part`.
    fn serie(&mut self, strate: Strate, serie: &[Calque], x: f32, part: f32) {
        let (k, suivant) = en_poses(x, serie.len());
        if let Some(c) = k.checked_sub(1).and_then(|i| serie.get(i)) {
            self.peindre(strate, c, part);
        }
        if let Some(c) = serie.get(k) {
            self.peindre(strate, c, suivant * part);
        }
    }

    /// Ses couches, dans l'ordre où elles se peignent : de la strate la plus basse à la plus haute.
    fn couches(&self) -> impl Iterator<Item = &Couche> {
        self.couches.iter().flatten()
    }
}

/// Où est la tête d'Eve, ses épaules ne bougeant pas : penchée (son roulis, en radians ; positif, vers son épaule
/// gauche, à droite à l'écran), tournée (son lacet ; positif, le visage vers la droite de l'écran), hochée (son
/// tangage ; positif, le menton vers le bas) et déplacée (une part du rayon du disque, en x et en y, y vers le bas). Le
/// rendu la tourne et la hoche en trois dimensions ([`rendu::Mouvement`]).
#[derive(Clone, Copy, Default, Debug)]
struct Tete {
    roulis: f32,
    lacet: f32,
    tangage: f32,
    x: f32,
    y: f32,
}

impl std::ops::Add for Tete {
    type Output = Tete;

    fn add(self, b: Tete) -> Tete {
        Tete {
            roulis: self.roulis + b.roulis,
            lacet: self.lacet + b.lacet,
            tangage: self.tangage + b.tangage,
            x: self.x + b.x,
            y: self.y + b.y,
        }
    }
}

impl std::ops::Mul<f32> for Tete {
    type Output = Tete;

    fn mul(self, k: f32) -> Tete {
        Tete { roulis: self.roulis * k, lacet: self.lacet * k, tangage: self.tangage * k, x: self.x * k, y: self.y * k }
    }
}

/// Où est le corps d'Eve sous sa tête : l'air dans sa poitrine ([`souffle`] : 0 au bout d'une expiration au repos, 1 au
/// haut d'une inspiration au repos), que le rendu montre en soulevant sa poitrine et ses épaules ; ses épaules levées
/// et son torse redressé (en part du rayon du disque ; négatif : plus bas) ; son torse penché du côté de la tête (en
/// radians, comme elle) et vers la personne (la part dont elle grandit ; négatif : en arrière).
#[derive(Clone, Copy, Default)]
struct Corps {
    souffle: f32,
    epaules: f32,
    monte: f32,
    roulis: f32,
    avance: f32,
}

/// Ce que fait Eve, pour son visage : au repos, elle écoute, elle réfléchit (ou transcrit), elle parle.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Activite {
    #[default]
    Repos,
    Ecoute,
    Reflechit,
    Parle,
}

impl Activite {
    /// L'activité d'un état d'Eve (celui que publie le chef : « parole », « ecoute », « reflexion »…).
    pub(crate) fn de(etat: &str) -> Activite {
        match etat {
            "parole" => Activite::Parle,
            "ecoute" => Activite::Ecoute,
            "reflexion" | "transcription" => Activite::Reflechit,
            _ => Activite::Repos,
        }
    }
}

/// Le portrait d'Eve, tel que la fenêtre le tient : son dossier local, le regard sur ses fichiers ([`Veille`]), le
/// visage prêt à peindre, l'heure du dernier regard, sa mimique, le suivi de sa netteté.
pub(crate) struct Portrait {
    local: PathBuf,
    veille: Arc<Mutex<Veille>>,
    visage: Option<Visage>,
    regarde_a: f64,
    mimique: mimique::Mimique,
    nettete: nettete::Suivi,
}

/// Le regard sur les fichiers du portrait, que porte un fil (sur le fil de la fenêtre, lister et dater sa centaine de
/// fichiers prenait une image entière toutes les deux secondes : la mimique sautait) : les fichiers vus (leur date et
/// leur taille), ce qu'il a lu et que la fenêtre n'a pas encore pris, s'il regarde en ce moment, et son tour — un
/// portrait retiré pendant une lecture ne revient pas avec elle.
#[derive(Default)]
struct Veille {
    vu: Vec<(PathBuf, SystemTime, u64)>,
    lu: Option<Lu>,
    en_cours: bool,
    tour: u64,
}

/// Ce que le regard a trouvé de neuf : plus de portrait, ou ses images relues (ou pourquoi elles ne se lisent pas).
enum Lu {
    Aucun,
    Images(Result<Images, String>),
}

impl Portrait {
    pub(crate) fn nouveau(local: PathBuf) -> Portrait {
        Portrait {
            local,
            veille: Arc::default(),
            visage: None,
            regarde_a: f64::NEG_INFINITY,
            mimique: mimique::Mimique::default(),
            nettete: nettete::Suivi::default(),
        }
    }

    /// Le portrait à cette image : ses fichiers regardés toutes les deux secondes, dans un fil, et relus quand l'un
    /// d'eux change, avec sa mise en place ([`mise_en_place`]). `None` : pas de portrait (ou illisible, et le journal le
    /// dit) : l'orbe reste.
    pub(crate) fn visage(&mut self, ctx: &egui::Context) -> Option<Visage> {
        let maintenant = ctx.input(|i| i.time);
        let (lu, libre) = match self.veille.lock() {
            Ok(mut v) => (v.lu.take(), !v.en_cours),
            Err(_) => (None, false),
        };
        if libre && maintenant - self.regarde_a >= 2.0 {
            self.regarde_a = maintenant;
            self.regarder(ctx);
        }
        if let Some(lu) = lu {
            self.visage = match lu {
                Lu::Aucun => None,
                Lu::Images(Ok(i)) => {
                    let texture =
                        |nom: String, image: &Arc<egui::ColorImage>| ctx.load_texture(nom, image.clone(), LISSE);
                    let calque = |nom: String, r: Recadre| {
                        let image = Arc::new(r.image);
                        Calque { texture: texture(nom, &image), image, uv: r.uv, geometrie: Arc::new(r.geometrie) }
                    };
                    let serie = |nom: &str, recadres: Vec<Recadre>| -> Vec<Calque> {
                        recadres.into_iter().enumerate().map(|(k, r)| calque(format!("{nom}-{}", k + 1), r)).collect()
                    };
                    let mut sortes = SORTES.iter();
                    let calques = i
                        .calques
                        .map(|recadres| serie(&format!("portrait-{}", sortes.next().copied().unwrap_or("")), recadres));
                    let expressions = i
                        .expressions
                        .into_iter()
                        .map(|(emotion, visage, bouches, closes)| {
                            let nom = format!("portrait-emotion-{}", emotion.nom());
                            let bouches = serie(&format!("{nom}-bouche"), bouches);
                            let levres_closes = closes.map(|r| calque(format!("{nom}-ferme"), r));
                            Expression { emotion, visage: calque(nom, visage), bouches, levres_closes }
                        })
                        .collect();
                    let calme = i.calme.map(|r| calque("portrait-calme".into(), r));
                    let photo = Arc::new(i.photo);
                    self.nettete = nettete::Suivi::default();
                    Some(Visage {
                        taille: photo.size,
                        texture: texture("portrait-d-eve".into(), &photo),
                        image: photo,
                        calques,
                        expressions,
                        calme,
                        cheveux: i.cheveux.map(Arc::new),
                        fond: i.detoure.then(|| rendu::fond_de_studio(ctx)),
                        cadre: i.cadre,
                        demande: Arc::default(),
                        net: None,
                        vignettes: Arc::default(),
                    })
                }
                Lu::Images(Err(e)) => {
                    crate::journal::ecrire(&format!("le portrait d'Eve ne se lit pas : {e}"));
                    None
                }
            };
        }
        if let Some(v) = self.visage.as_mut() {
            self.nettete.suivre(ctx, v, maintenant);
        }
        self.visage.clone()
    }

    /// Un regard sur les fichiers du portrait, dans un fil : leur date et leur taille ; s'ils ont changé, le portrait
    /// relu (ou plus de portrait), rendu à la fenêtre au prochain [`Portrait::visage`].
    fn regarder(&self, ctx: &egui::Context) {
        let Ok(mut v) = self.veille.lock() else { return };
        v.en_cours = true;
        drop(v);
        let (veille, ctx, local) = (self.veille.clone(), ctx.clone(), self.local.clone());
        std::thread::spawn(move || {
            let vus: Vec<_> = fichiers(&local)
                .into_iter()
                .filter_map(|c| {
                    let m = std::fs::metadata(&c).ok()?;
                    Some((c, m.modified().ok()?, m.len()))
                })
                .collect();
            let tour = match veille.lock() {
                Ok(mut v) if v.vu != vus => {
                    v.vu = vus.clone();
                    v.tour
                }
                Ok(mut v) => {
                    v.en_cours = false;
                    return;
                }
                Err(_) => return,
            };
            let lu = if vus.is_empty() { Lu::Aucun } else { Lu::Images(lire(&local)) };
            if let Ok(mut v) = veille.lock() {
                if v.tour == tour {
                    v.lu = Some(lu);
                }
                v.en_cours = false;
            }
            ctx.request_repaint();
        });
    }

    /// La pose du visage en grand à l'instant `temps` (celui de la fenêtre), Eve à son `activite`, l'émotion de sa
    /// réponse `emotion` ([`Emotion`]) : la mimique ([`mimique`]) — quand Eve parle, sa bouche suit sa voix ; elle cligne
    /// et regarde de côté de temps en temps ; sa tête prend une posture et hoche ; son visage prend l'émotion en fondu.
    /// `vivant` : les animations de Windows sont permises — sans elles, ni souffle, ni regard de côté, ni tête qui bouge,
    /// et elle cligne encore. `None` : pas de portrait.
    pub(crate) fn pose_du_moment(
        &mut self,
        temps: f64,
        activite: Activite,
        vivant: bool,
        emotion: Option<Emotion>,
    ) -> Option<Pose> {
        let v = self.visage.as_ref()?;
        Some(self.mimique.pose(v, temps, activite, vivant, emotion))
    }

    /// La voix de la personne que le micro entend (de 0 à 1 ; 0 quand Eve n'écoute pas), pour les poses qui suivent :
    /// en l'écoutant, Eve hoche à ses pauses.
    pub(crate) fn entendre(&mut self, niveau: f32) {
        self.mimique.entendre(niveau);
    }

    /// Le dossier local, où vit le portrait.
    pub(crate) fn local(&self) -> &Path {
        &self.local
    }

    /// Retire le portrait : l'orbe revient tout de suite, sans attendre le prochain regard sur les fichiers ; une lecture
    /// en cours ne le ramène pas.
    pub(crate) fn retirer(&mut self) {
        retirer(&self.local);
        self.visage = None;
        if let Ok(mut v) = self.veille.lock() {
            (v.vu, v.lu, v.tour) = (Vec::new(), None, v.tour + 1);
        }
    }
}

/// Une série de `n` calques de plus en plus marqués, à l'avance `x` (de 0 à 1) : le nombre de ceux qui sont passés —
/// celui d'en dessous est le dernier d'entre eux ; 0 : aucun, la photo — et la part du suivant qui se fond en lui.
fn en_poses(x: f32, n: usize) -> (usize, f32) {
    let x = x.clamp(0.0, 1.0) * n as f32;
    let k = (x.floor() as usize).min(n);
    (k, if k < n { x - k as f32 } else { 0.0 })
}

/// Lit le portrait du dossier local, dans un fil : la photo, sa mise en place, et les calques de ses poses de sa taille
/// (une autre est laissée, et le journal le dit), recadrés, chacun avec sa géométrie ([`rendu::geometrie`]) ; ses
/// émotions, leurs bouches et leurs lèvres closes ; son visage calme ; détourée, la souplesse de ses cheveux, d'après
/// leur masque ([`cheveux::Souplesse`]). Les poses se lisent à plusieurs fils à la fois ([`en_parallele`]) : le flot de
/// chacune prend son temps.
fn lire(local: &Path) -> Result<Images, String> {
    let debut = Instant::now();
    let chemin = trouver(local, "portrait").ok_or("le portrait a disparu")?;
    let photo = crate::cartes::image_du_disque(&chemin, COTE)?;
    let (detoure, cadre) = mise_en_place(&photo);
    let [l, h] = photo.size;
    let lue = |c: &Path| match crate::cartes::image_du_disque(c, COTE) {
        Ok(i) if i.size == photo.size => Some(i),
        Ok(_) => {
            crate::journal::ecrire(&format!("portrait d'Eve : {} n'a pas la taille de la photo", c.display()));
            None
        }
        Err(e) => {
            crate::journal::ecrire(&format!("portrait d'Eve : {} ne se lit pas : {e}", c.display()));
            None
        }
    };
    // Le calque d'une pose `i` : ce qui y change par rapport à ce qu'elle recouvre (`dessous` : la photo, ou le visage
    // de son émotion), son masque élargi de la part `elargi` de l'image, recadré ; et sa géométrie, le flot de la photo
    // vers la pose — le treillis va toujours de la photo à ce qui se voit.
    let pose = |i: &egui::ColorImage, dessous: &egui::ColorImage, elargi: usize| {
        let (image, masque) = calque(dessous, i, elargi);
        let (image, [x0, y0, x1, y1]) = recadrer(image);
        let geometrie = if image.size == [1, 1] {
            rendu::Geometrie::default()
        } else {
            rendu::geometrie(&photo, i, &masque, [x0, y0, x1, y1])
        };
        let (lf, hf) = (l as f32, h as f32);
        let uv = Rect::from_min_max(pos2(x0 as f32 / lf, y0 as f32 / hf), pos2(x1 as f32 / lf, y1 as f32 / hf));
        Recadre { image, uv, geometrie }
    };
    let series = SORTES.map(|sorte| poses(local, "portrait", sorte));
    let calme = trouver(local, "portrait-calme");
    let emotions: Vec<(Emotion, PathBuf, Vec<PathBuf>, Option<PathBuf>)> = emotions(local, "portrait")
        .into_iter()
        .map(|(e, c)| (e, c, bouches_d_emotion(local, "portrait", e), levres_closes_d_emotion(local, "portrait", e)))
        .collect();
    // Une émotion : son visage, ce qui y change par rapport à la photo ; ses bouches et ses lèvres closes, ce qui y
    // change par rapport à son visage — la bouche seule. Posées sur lui pour parler, elles ne reprennent ni ses yeux ni
    // ses sourcils : le visage ne saute pas à la première syllabe, et un regard de côté, dessous, ne va et vient pas
    // d'une syllabe à l'autre. Les visages d'abord, donc.
    let visages: Vec<Option<(egui::ColorImage, Recadre)>> = en_parallele(&emotions, |(_, c, _, _)| {
        let i = lue(c)?;
        let r = pose(&i, &photo, ELARGI);
        Some((i, r))
    })
    .into_iter()
    .map(Option::flatten)
    .collect();
    // Puis toutes les autres, dans l'ordre, chacune avec l'élargissement de son masque et ce qu'elle recouvre : les
    // séries de chaque sorte, puis les bouches et les lèvres closes de chaque émotion dont le visage se lit, puis le
    // visage calme.
    let sur_la_photo = &photo;
    let chemins: Vec<(&PathBuf, usize, &egui::ColorImage)> = SORTES
        .iter()
        .zip(&series)
        .flat_map(|(sorte, s)| {
            let elargi = if ETROITES.contains(sorte) { ELARGI_DES_YEUX } else { ELARGI };
            s.iter().map(move |c| (c, elargi, sur_la_photo))
        })
        .chain(emotions.iter().zip(&visages).flat_map(|((_, _, bouches, closes), v)| {
            let dessous = v.as_ref().map(|(i, _)| i);
            bouches.iter().chain(closes).filter_map(move |b| Some((b, ELARGI, dessous?)))
        }))
        .chain(calme.iter().map(|c| (c, ELARGI, sur_la_photo)))
        .collect();
    let mut lues = en_parallele(&chemins, |&(c, elargi, dessous)| Some(pose(&lue(c)?, dessous, elargi)))
        .into_iter()
        .map(Option::flatten);
    let n = emotions.len() + chemins.len();
    // Chaque série s'arrête à sa première pose illisible.
    let mut suite = |n: usize| -> Vec<Recadre> {
        let lues: Vec<Option<Recadre>> = lues.by_ref().take(n).collect();
        lues.into_iter().map_while(|r| r).collect()
    };
    let calques = series.each_ref().map(|s| suite(s.len()));
    let expressions = emotions
        .iter()
        .zip(visages)
        .filter_map(|((e, _, bouches, closes), v)| {
            let (_, visage) = v?;
            let bouches = suite(bouches.len());
            let closes = suite(usize::from(closes.is_some())).pop();
            Some((*e, visage, bouches, closes))
        })
        .collect();
    let calme = suite(usize::from(calme.is_some())).pop();
    let cheveux = trouver(local, "portrait-cheveux")
        .filter(|_| detoure)
        .and_then(|c| lue(&c))
        .map(|masque| cheveux::Souplesse::de(&photo, &masque));
    let et_cheveux = if cheveux.is_some() { " et le masque des cheveux" } else { "" };
    let duree = debut.elapsed().as_secs_f32();
    crate::journal::ecrire(&format!("portrait d'Eve lu : la photo, {n} pose(s){et_cheveux}, en {duree:.1} s"));
    Ok(Images { photo, calques, expressions, calme, cheveux, detoure, cadre })
}

/// `f` sur chaque entrée, à plusieurs fils à la fois — trois au plus, et un cœur laissé à la fenêtre —, les sorties dans
/// l'ordre des entrées ; celles d'un fil qui panique restent à `None`.
fn en_parallele<E: Sync, S: Send>(entrees: &[E], f: impl Fn(&E) -> S + Sync) -> Vec<Option<S>> {
    let fils = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1)).clamp(1, 3);
    let suivante = AtomicUsize::new(0);
    let faites: Vec<Vec<(usize, S)>> = std::thread::scope(|s| {
        let fils: Vec<_> = (0..fils)
            .map(|_| {
                s.spawn(|| {
                    let mut faites = Vec::new();
                    loop {
                        let k = suivante.fetch_add(1, Ordering::Relaxed);
                        let Some(e) = entrees.get(k) else { break faites };
                        faites.push((k, f(e)));
                    }
                })
            })
            .collect();
        fils.into_iter().filter_map(|f| f.join().ok()).collect()
    });
    let mut sorties: Vec<Option<S>> = (0..entrees.len()).map(|_| None).collect();
    for (k, s) in faites.into_iter().flatten() {
        sorties[k] = Some(s);
    }
    sorties
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

/// Les bouches d'une émotion pour parler, posées dans `dossier` à côté de l'image `base`
/// (`base-emotion-triste-bouche-1`…), de plus en plus ouvertes.
fn bouches_d_emotion(dossier: &Path, base: &str, e: Emotion) -> Vec<PathBuf> {
    poses(dossier, base, &format!("emotion-{}-bouche", e.nom()))
}

/// L'élargissement du masque d'un calque (son rayon : le grand côté de l'image divisé par ce nombre) : un soixantième,
/// pour que le bord d'une pose ne se voie pas ; un cent-vingtième pour les sortes au masque étroit, les yeux et les
/// regards — l'écart des yeux aux sourcils est d'une dizaine de pixels, et un masque plus large peignait, sur des
/// sourcils levés, ceux de la photo : un sourcil fantôme (trouvé par l'agent du regard, ses poses des yeux ne changeant
/// que les yeux).
const ELARGI: usize = 60;
const ELARGI_DES_YEUX: usize = 120;
const ETROITES: [&str; 2] = ["yeux", "regard"];

/// Les lèvres closes d'une émotion, posées dans `dossier` à côté de l'image `base` (`base-emotion-joie-ferme`), si elle
/// en a.
fn levres_closes_d_emotion(dossier: &Path, base: &str, e: Emotion) -> Option<PathBuf> {
    trouver(dossier, &format!("{base}-emotion-{}-ferme", e.nom()))
}

/// Le calque d'une pose : ce qui y change par rapport à la photo — plus de 10 sur 255 d'écart —, élargi et adouci sur
/// la part `elargi` de l'image ([`ELARGI`]) pour que son bord ne se voie pas, le reste transparent ; et ce masque (de 0
/// à 1, un par pixel). Un écart isolé (le grain de la photo) ne compte pas.
fn calque(photo: &egui::ColorImage, pose: &egui::ColorImage, elargi: usize) -> (egui::ColorImage, Vec<f32>) {
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
    let r = (l.max(h) / elargi.max(1)).max(2);
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
    (egui::ColorImage::new([l, h], pixels), masque)
}

/// Le calque recadré sur ce qui s'y voit : la plus petite part de l'image qui garde ses pixels non transparents, plus
/// une [`MARGE`] transparente, dans l'image — une bouche ne pèse plus qu'une bouche dans la carte graphique ; et cette
/// part, en pixels de l'image entière (gauche, haut, droite, bas). Rien ne s'y voit : un seul pixel transparent, sur
/// toute l'image.
fn recadrer(c: egui::ColorImage) -> (egui::ColorImage, [usize; 4]) {
    let [l, h] = c.size;
    let (mut x0, mut y0, mut x1, mut y1) = (l, h, 0, 0);
    for (i, p) in c.pixels.iter().enumerate() {
        if p.a() > 0 {
            let (x, y) = (i % l, i / l);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        }
    }
    if x0 >= x1 {
        return (egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]), [0, 0, l, h]);
    }
    let (x0, y0) = (x0.saturating_sub(MARGE), y0.saturating_sub(MARGE));
    let (x1, y1) = ((x1 + MARGE).min(l), (y1 + MARGE).min(h));
    let pixels = (y0..y1).flat_map(|y| c.pixels[y * l + x0..y * l + x1].iter().copied()).collect();
    (egui::ColorImage::new([x1 - x0, y1 - y0], pixels), [x0, y0, x1, y1])
}

/// Un flou en boîte de rayon `r` sur une image d'un seul canal (`l` sur `h`), sur place : les rangées, chacune par ses
/// sommes cumulées ; puis les colonnes, toutes ensemble, rangée après rangée — la somme des rangées de la fenêtre,
/// glissée d'une rangée à la suivante : la mémoire se lit dans l'ordre (colonne par colonne, chaque pixel lu sautait
/// d'une rangée, et le calque d'une pose prenait des dizaines de millisecondes).
fn flou(m: &mut [f32], l: usize, h: usize, r: usize) {
    if l == 0 || h == 0 {
        return;
    }
    let mut ligne = Vec::with_capacity(l + 1);
    for rang in m.chunks_exact_mut(l) {
        ligne.clear();
        ligne.push(0.0f32);
        for i in 0..l {
            let s = ligne[i] + rang[i];
            ligne.push(s);
        }
        for (i, v) in rang.iter_mut().enumerate() {
            let (a, b) = (i.saturating_sub(r), (i + r + 1).min(l));
            *v = (ligne[b] - ligne[a]) / (b - a) as f32;
        }
    }
    let source = m.to_vec();
    let rangee = |y: usize| &source[y * l..y * l + l];
    let mut somme = vec![0.0f32; l];
    for y in 0..(r + 1).min(h) {
        somme.iter_mut().zip(rangee(y)).for_each(|(s, v)| *s += v);
    }
    for (y, sortie) in m.chunks_exact_mut(l).enumerate() {
        let n = ((y + r + 1).min(h) - y.saturating_sub(r)) as f32;
        sortie.iter_mut().zip(&somme).for_each(|(o, s)| *o = s / n);
        if y + r + 1 < h {
            somme.iter_mut().zip(rangee(y + r + 1)).for_each(|(s, v)| *s += v);
        }
        if y >= r {
            somme.iter_mut().zip(rangee(y - r)).for_each(|(s, v)| *s -= v);
        }
    }
}

impl Visage {
    /// Chaque pose, nommée, pour l'essai du portrait : la photo, chaque calque seul (de chaque sorte, son nom et son
    /// rang), le visage calme (seul, puis qui parle), deux fondus (une bouche à moitié fondue dans la suivante ; les
    /// lèvres à moitié closes), trois mélanges — les sourcils levés en clignant, cligner en parlant, regarder de côté
    /// en parlant —, chaque émotion (seule, puis qui parle, sa bouche du milieu, puis ses lèvres closes si elle les
    /// a), des passages à mi-chemin (un clignement, chaque émotion qui vient, le visage calme), la tête penchée,
    /// tournée, baissée, relevée, le buste, et les cheveux en retard sur la tête. Les calques de chacune se posent dans
    /// l'ordre où ils sont nommés.
    pub(crate) fn poses_d_essai(&self) -> Vec<(String, Pose)> {
        let [yeux, regards, ouvertes, _, _, fermees, sourcils] = &self.calques;
        let pose = |calques: &[(&Calque, f32)]| {
            let mut p = Pose::default();
            for (c, part) in calques {
                p.peindre(Strate::Dessous, c, *part);
            }
            p
        };
        let mut v = vec![("la photo".to_string(), Pose::default())];
        for (sorte, calques) in SORTES.iter().zip(&self.calques) {
            for (k, c) in calques.iter().enumerate() {
                v.push((format!("{sorte} {}", k + 1), pose(&[(c, 1.0)])));
            }
        }
        if let Some(c) = &self.calme {
            v.push(("calme".into(), pose(&[(c, 1.0)])));
            if let Some(b) = ouvertes.get(ouvertes.len() / 2) {
                v.push(("calme, qui parle".into(), pose(&[(c, 1.0), (b, 1.0)])));
            }
        }
        if let [a, b, ..] = ouvertes.as_slice() {
            v.push(("bouche 1 et 2, en fondu".into(), pose(&[(a, 1.0), (b, 0.5)])));
        }
        if let (Some(s), Some(y)) = (sourcils.last(), yeux.last()) {
            v.push(("sourcils levés, en clignant".into(), pose(&[(s, 1.0), (y, 1.0)])));
        }
        if let (Some(b), Some(f)) = (ouvertes.get(2), fermees.first()) {
            v.push(("lèvres à demi closes".into(), pose(&[(b, 1.0), (f, 0.5)])));
        }
        if let (Some(b), Some(y)) = (ouvertes.get(ouvertes.len() / 2), yeux.last()) {
            v.push(("cligne en parlant".into(), pose(&[(b, 1.0), (y, 1.0)])));
        }
        if let (Some(b), Some(r)) = (ouvertes.last(), regards.first()) {
            v.push(("regarde en parlant".into(), pose(&[(r, 1.0), (b, 1.0)])));
        }
        for x in &self.expressions {
            v.push((format!("émotion : {}", x.emotion.nom()), pose(&[(&x.visage, 1.0)])));
            if let Some(b) = x.bouches.get(x.bouches.len() / 2) {
                v.push((format!("{}, qui parle", x.emotion.nom()), pose(&[(&x.visage, 1.0), (b, 1.0)])));
            }
            if let Some(f) = &x.levres_closes {
                v.push((format!("{}, lèvres closes", x.emotion.nom()), pose(&[(&x.visage, 1.0), (f, 1.0)])));
            }
        }
        // À mi-chemin d'un passage, où deux poses se fondent l'une dans l'autre (le rendu les fait tomber l'une sur
        // l'autre) : un clignement, chaque émotion qui vient.
        if let [.., a, b] = yeux.as_slice() {
            v.push(("clignement, à mi-chemin".into(), pose(&[(a, 1.0), (b, 0.5)])));
        }
        for x in &self.expressions {
            v.push((format!("{}, à mi-chemin", x.emotion.nom()), pose(&[(&x.visage, 0.5)])));
        }
        if let Some(c) = &self.calme {
            v.push(("calme, à mi-chemin".into(), pose(&[(c, 0.5)])));
        }
        // La tête penchée, plus que jamais en vrai (le cou et la jonction avec les épaules s'y jugent), et en parlant.
        let penchee = Tete { roulis: 0.08, x: 0.03, y: 0.02, ..Tete::default() };
        v.push(("tête penchée".into(), Pose { tete: penchee, ..Pose::default() }));
        let autre = Tete { roulis: -0.08, x: -0.03, y: -0.01, ..Tete::default() };
        let bouche: Vec<_> = ouvertes.last().map(|b| (b, 1.0)).into_iter().collect();
        v.push(("penchée, en parlant".into(), Pose { tete: autre, ..pose(&bouche) }));
        // La tête tournée, baissée, relevée, de cinq degrés — plus que jamais en vrai : le visage doit y rester un
        // visage (le nez qui avance plus que les joues, le côté qui s'éloigne resserré), sans pli au cou, aux cheveux
        // ni au bord.
        let tete = |roulis, lacet, tangage| Tete { roulis, lacet, tangage, ..Tete::default() };
        v.push(("tête tournée".into(), Pose { tete: tete(0.0, 0.09, 0.0), ..Pose::default() }));
        v.push(("tournée de l'autre côté, en parlant".into(), Pose { tete: tete(0.0, -0.09, 0.0), ..pose(&bouche) }));
        v.push(("tête baissée".into(), Pose { tete: tete(0.0, 0.0, 0.09), ..Pose::default() }));
        v.push(("tête relevée".into(), Pose { tete: tete(0.0, 0.0, -0.09), ..Pose::default() }));
        v.push(("tournée, penchée et baissée".into(), Pose { tete: tete(0.05, 0.07, 0.05), ..Pose::default() }));
        // Une inspiration bien plus profonde que jamais en vrai, la tête penchée ; les épaules haussées, puis tombées,
        // bien plus que jamais en vrai : le cou, les épaules et le bord du disque s'y jugent.
        let corps = Corps { souffle: 3.0, ..Corps::default() };
        v.push(("inspiration profonde".into(), Pose { tete: penchee, corps, ..Pose::default() }));
        let haussees = Corps { epaules: 0.02, ..Corps::default() };
        v.push(("épaules haussées".into(), Pose { tete: autre, corps: haussees, ..Pose::default() }));
        let tombees = Corps { epaules: -0.012, souffle: -0.3, ..Corps::default() };
        v.push(("épaules tombées".into(), Pose { corps: tombees, ..Pose::default() }));
        // Le torse penché du côté de la tête et vers la personne, bien plus que jamais en vrai.
        let torse = Corps { roulis: 0.04, avance: 0.025, ..Corps::default() };
        v.push(("torse penché".into(), Pose { tete: penchee, corps: torse, ..Pose::default() }));
        // Les longueurs des cheveux en retard sur la tête penchée, deux fois plus que jamais en vrai : le visage, la
        // joue et le bord du disque s'y jugent.
        if let Some(s) = &self.cheveux {
            let cheveux = cheveux::Retard::d_essai(s.clone(), -0.015, -0.03);
            v.push(("cheveux en retard".into(), Pose { tete: penchee, cheveux, ..Pose::default() }));
        }
        v
    }
}

// Une image du portrait dans `dossier` (`nom` : « portrait », « portrait-yeux-1 »…), sous la première de ses
/// [`EXTENSIONS`] qui existe.
fn trouver(dossier: &Path, nom: &str) -> Option<PathBuf> {
    EXTENSIONS.iter().map(|e| dossier.join(format!("{nom}.{e}"))).find(|c| c.is_file())
}

/// Les poses d'une [sorte](SORTES) posées dans `dossier` à côté de l'image `base` : `base-yeux-1`, `base-yeux-2`…,
/// jusqu'à la première qui manque.
fn poses(dossier: &Path, base: &str, sorte: &str) -> Vec<PathBuf> {
    (1..=POSES_MAX).map_while(|k| trouver(dossier, &format!("{base}-{sorte}-{k}"))).collect()
}

/// Les fichiers du portrait dans le dossier local : la photo, puis ses poses, puis le masque de ses cheveux ; rien sans
/// photo.
fn fichiers(local: &Path) -> Vec<PathBuf> {
    let Some(photo) = trouver(local, "portrait") else { return Vec::new() };
    let des_emotions = emotions(local, "portrait").into_iter().flat_map(|(e, c)| {
        let closes = levres_closes_d_emotion(local, "portrait", e);
        std::iter::once(c).chain(bouches_d_emotion(local, "portrait", e)).chain(closes)
    });
    let series = SORTES.iter().flat_map(|s| poses(local, "portrait", s));
    let calme = trouver(local, "portrait-calme");
    let cheveux = trouver(local, "portrait-cheveux");
    std::iter::once(photo).chain(series).chain(des_emotions).chain(calme).chain(cheveux).collect()
}

/// Pose `choisie` comme portrait d'Eve : une image que Windows sait lire, copiée dans le dossier local sous son nom de
/// portrait (l'image d'origine peut ensuite bouger), avec les poses et le masque des cheveux posés à côté d'elle sous
/// son nom (« eve.png », « eve-yeux-1.png », « eve-bouche-1.png »…, « eve-cheveux.png ») ; le portrait d'avant et ses
/// poses s'en vont.
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
        for (k, b) in bouches_d_emotion(dossier, &base, e).iter().enumerate() {
            copier(b, local, &format!("portrait-emotion-{}-bouche-{}", e.nom(), k + 1))?;
            n += 1;
        }
        if let Some(f) = levres_closes_d_emotion(dossier, &base, e) {
            copier(&f, local, &format!("portrait-emotion-{}-ferme", e.nom()))?;
            n += 1;
        }
    }
    if let Some(c) = trouver(dossier, &format!("{base}-calme")) {
        copier(&c, local, "portrait-calme")?;
        n += 1;
    }
    if let Some(c) = trouver(dossier, &format!("{base}-cheveux")) {
        copier(&c, local, "portrait-cheveux")?;
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

/// Retire le portrait d'Eve du dossier local, ses poses et le masque de ses cheveux, sous tous leurs noms.
fn retirer(local: &Path) {
    let noms = SORTES.iter().flat_map(|s| (1..=POSES_MAX).map(move |k| format!("portrait-{s}-{k}")));
    let des_emotions = Emotion::TOUTES.iter().flat_map(|e| {
        let nom = format!("portrait-emotion-{}", e.nom());
        let bouches: Vec<String> = (1..=POSES_MAX).map(|k| format!("{nom}-bouche-{k}")).collect();
        let closes = format!("{nom}-ferme");
        std::iter::once(nom).chain(bouches).chain([closes])
    });
    let a_part = ["portrait-calme", "portrait-cheveux"].map(String::from);
    for nom in std::iter::once("portrait".to_string()).chain(noms).chain(des_emotions).chain(a_part) {
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
        // Une émotion, sa bouche et ses lèvres closes ; sans sa pose, ni sa bouche ni ses lèvres ne comptent. Le visage
        // calme, le masque des cheveux.
        for nom in [
            "portrait-emotion-triste.png",
            "portrait-emotion-triste-bouche-1.png",
            "portrait-emotion-triste-ferme.png",
            "portrait-emotion-joie-bouche-1.png",
            "portrait-emotion-joie-ferme.png",
            "portrait-calme.png",
            "portrait-cheveux.png",
        ] {
            std::fs::write(local.join(nom), b"x").unwrap();
        }
        let noms: Vec<String> =
            fichiers(&local).iter().map(|c| c.file_name().unwrap().to_string_lossy().into_owned()).collect();
        let attendus = ["portrait.jpg", "portrait-yeux-1.png", "portrait-yeux-2.png", "portrait-bouche-1.png"];
        let emotion = [
            "portrait-emotion-triste.png",
            "portrait-emotion-triste-bouche-1.png",
            "portrait-emotion-triste-ferme.png",
        ];
        let a_part = ["portrait-calme.png", "portrait-cheveux.png"];
        assert_eq!(noms, [attendus.as_slice(), &emotion, &a_part].concat());
        // Une image choisie ailleurs, et ses poses à côté d'elle, sous son nom : copiées sous ceux du portrait.
        let ailleurs = local.join("ailleurs");
        std::fs::create_dir_all(&ailleurs).unwrap();
        std::fs::write(ailleurs.join("eve-bouche-1.webp"), b"x").unwrap();
        assert_eq!(poses(&ailleurs, "eve", "bouche"), [ailleurs.join("eve-bouche-1.webp")]);
        assert!(poses(&ailleurs, "eve", "yeux").is_empty());
        let copie = copier(&ailleurs.join("eve-bouche-1.webp"), &local, "portrait-bouche-2").unwrap();
        assert_eq!(copie, local.join("portrait-bouche-2.webp"));
        assert_eq!(fichiers(&local).len(), 11);
        retirer(&local);
        assert!(fichiers(&local).is_empty());
        assert!(
            std::fs::read_dir(&local).unwrap().all(|e| e.unwrap().path() == ailleurs),
            "tout le portrait est parti"
        );
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn une_serie_se_fond_d_une_pose_a_l_autre() {
        // Trois calques, de plus en plus marqués : rien, aucun ; puis celui d'en dessous et la part du suivant qui se fond
        // en lui ; tout, le dernier.
        let k = [0.0, 0.2, 0.5, 1.0].map(|f| en_poses(f, 3));
        assert_eq!(k.map(|(k, _)| k), [0, 0, 1, 3]);
        assert!((k[1].1 - 0.6).abs() < 1e-6 && (k[2].1 - 0.5).abs() < 1e-6 && k[3].1 == 0.0, "{k:?}");
        assert_eq!(en_poses(1.0, 0), (0, 0.0));
        // L'activité d'Eve, d'après l'état que publie le chef.
        assert!(Activite::de("parole") == Activite::Parle && Activite::de("transcription") == Activite::Reflechit);
        assert!(Activite::de("ecoute") == Activite::Ecoute && Activite::de("pret") == Activite::Repos);
    }

    #[test]
    fn un_calque_se_recadre_sur_ce_qui_se_voit() {
        // 200 sur 100, transparent, sauf un carré de 10 en (100, 40) : recadré à lui et sa marge, dans l'image.
        let mut c = egui::ColorImage::new([200, 100], vec![Color32::TRANSPARENT; 200 * 100]);
        for y in 40..50 {
            for x in 100..110 {
                c.pixels[y * 200 + x] = Color32::from_rgb(10, 20, 30);
            }
        }
        let (image, part) = recadrer(c);
        let (x0, y0) = (100 - MARGE, 40 - MARGE);
        assert_eq!(image.size, [10 + 2 * MARGE, 10 + 2 * MARGE]);
        assert_eq!(image.pixels[MARGE * image.size[0] + MARGE], Color32::from_rgb(10, 20, 30));
        assert_eq!(part, [x0, y0, 110 + MARGE, 50 + MARGE]);
        // Un point de l'image entière se retrouve dans le calque : le coin du carré, au bout de la marge.
        let uv = Rect::from_min_max(pos2(x0 as f32 / 200.0, y0 as f32 / 100.0), pos2(142.0 / 200.0, 82.0 / 100.0));
        let couche = Couche { texture: egui::TextureId::default(), uv, part: 1.0, geometrie: None };
        let q = couche.dans(pos2(100.0 / 200.0, 40.0 / 100.0));
        assert!((q.x * image.size[0] as f32 - MARGE as f32).abs() < 1e-3, "{q:?}");
        // Contre le bord de l'image, la marge s'arrête à lui ; rien de visible : un pixel transparent, sur toute l'image.
        let mut bord = egui::ColorImage::new([50, 50], vec![Color32::TRANSPARENT; 50 * 50]);
        bord.pixels[0] = Color32::WHITE;
        assert_eq!(recadrer(bord).0.size, [1 + MARGE, 1 + MARGE]);
        let (vide, part) = recadrer(egui::ColorImage::new([50, 50], vec![Color32::TRANSPARENT; 50 * 50]));
        assert_eq!((vide.size, part), ([1, 1], [0, 0, 50, 50]));
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
        let (c, masque) = calque(&photo, &pose, ELARGI);
        let alpha = |x: usize, y: usize| c.pixels[y * 120 + x].a();
        assert_eq!(alpha(55, 35), 255, "le carré, entier");
        assert!(alpha(48, 35) > 0, "et un peu autour, adouci");
        assert_eq!(alpha(10, 100), 0, "le pixel seul ne compte pas");
        assert_eq!(alpha(100, 100), 0, "le reste, transparent");
        assert_eq!(c.pixels[35 * 120 + 55], Color32::from_rgb(60, 40, 30), "la couleur de la pose");
        // Le masque : la part de la pose qui se voit, pixel par pixel.
        assert!(masque[35 * 120 + 55] == 1.0 && masque[100 * 120 + 100] == 0.0);
        assert!((masque[35 * 120 + 48] - f32::from(alpha(48, 35)) / 255.0).abs() < 0.01);
        // Étroit (les yeux), le masque s'arrête plus près de ce qui change : il ne déborde pas sur les sourcils. Une photo
        // de 480 sur 480, un carré de 20 qui change en (230, 200) : au-dessus de lui, le masque s'étend deux fois moins.
        let grande = egui::ColorImage::new([480, 480], vec![gris; 480 * 480]);
        let mut pose = grande.clone();
        for y in 200..220 {
            for x in 230..250 {
                pose.pixels[y * 480 + x] = Color32::from_rgb(60, 40, 30);
            }
        }
        let portee = |elargi: usize| {
            let (_, m) = calque(&grande, &pose, elargi);
            (0..200).find(|&y| m[y * 480 + 240] > 0.0).map_or(0, |y| 200 - y)
        };
        let (large, etroite) = (portee(ELARGI), portee(ELARGI_DES_YEUX));
        assert!(etroite > 0 && 2 * etroite <= large + 2, "{etroite} {large}");
        // Le flou en boîte : chaque pixel, la moyenne de sa fenêtre, coupée aux bords de l'image.
        let (l, h, r) = (9, 7, 2);
        let image: Vec<f32> = (0..l * h).map(|i| ((i * 37) % 11) as f32).collect();
        let mut floue = image.clone();
        flou(&mut floue, l, h, r);
        let image = &image;
        for y in 0..h {
            for x in 0..l {
                let (xs, ys) = (x.saturating_sub(r)..(x + r + 1).min(l), y.saturating_sub(r)..(y + r + 1).min(h));
                let n = (xs.len() * ys.len()) as f32;
                let attendu: f32 = ys.flat_map(|yy| xs.clone().map(move |xx| image[yy * l + xx])).sum::<f32>() / n;
                assert!((floue[y * l + x] - attendu).abs() < 1e-4, "({x}, {y}) : {} {attendu}", floue[y * l + x]);
            }
        }
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
