//! Le portrait d'Eve, dans le thème « Constructo AI » (demandé le 25 octobre : « une photo IA (EVE) à la place de
//! l'orbe », puis « avec un fond transparent ») : une image que la personne choisit, à la place de l'orbe — dans la
//! bannière du tableau de bord, en tête de la conversation seule, dans l'en-tête du dock et devant les bulles d'Eve —,
//! cernée d'un anneau qui dit ce qu'elle fait, comme l'orbe le montrait. Une photo se montre en disque ; un portrait
//! détouré (un PNG au fond transparent) se pose sur un disque clair, comme sous la lumière d'un studio, et en grand, sa
//! tête sort du cercle par le haut. Sa seule source : `portrait.png` (ou `.jpg`, `.jpeg`, `.webp`, `.gif`) du dossier
//! local, que les réglages posent ou retirent et que la fenêtre relit quand il change. Sans lui, l'orbe reste.

use std::f32::consts::{FRAC_PI_4, PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2, vec2};

use super::theme::erp;

/// Les noms que peut porter le portrait, dans le dossier local.
const NOMS: [&str; 5] = ["portrait.png", "portrait.jpg", "portrait.jpeg", "portrait.webp", "portrait.gif"];
/// Le grand côté du portrait chargé, en pixels : de quoi la bannière d'un écran 4K.
const COTE: usize = 768;
/// Ce que la tête d'un portrait détouré dépasse du cercle, en grand : une part du diamètre.
const DEPASSE: f32 = 0.10;
/// Le visage de plus près, dans les petits disques : le grossissement, et la hauteur du visage dans le disque de la
/// grande vue (une part de son diamètre, depuis son haut).
const DE_PRES: f32 = 1.6;
const HAUTEUR_DU_VISAGE: f32 = 0.3;

/// Le portrait prêt à peindre : sa texture ; détouré, le fond de son disque ; le carré de l'image (en coordonnées de
/// texture, de 0 à 1) que couvre le disque de la grande vue.
#[derive(Clone)]
pub(crate) struct Visage {
    texture: egui::TextureHandle,
    fond: Option<egui::TextureHandle>,
    cadre: Rect,
}

/// Ce que rend le fil de lecture : l'image, détourée ou non, et le carré de son disque.
type Lu = Result<(egui::ColorImage, bool, Rect), String>;

/// Le portrait d'Eve, tel que la fenêtre le tient : son dossier local, le fichier vu (sa date et sa taille), le visage
/// prêt à peindre, sa lecture en cours.
pub(crate) struct Portrait {
    local: PathBuf,
    vu: Option<(PathBuf, SystemTime, u64)>,
    visage: Option<Visage>,
    lu: Arc<Mutex<Option<Lu>>>,
    en_lecture: bool,
    regarde_a: f64,
}

impl Portrait {
    pub(crate) fn nouveau(local: PathBuf) -> Portrait {
        Portrait { local, vu: None, visage: None, lu: Arc::default(), en_lecture: false, regarde_a: f64::NEG_INFINITY }
    }

    /// Le portrait à cette image : son fichier regardé toutes les deux secondes, relu dans un fil quand il change, avec
    /// sa mise en place ([`mise_en_place`]). `None` : pas de portrait (ou illisible, et le journal le dit) : l'orbe
    /// reste.
    pub(crate) fn visage(&mut self, ctx: &egui::Context) -> Option<Visage> {
        let maintenant = ctx.input(|i| i.time);
        if let Some(lu) = self.lu.lock().ok().and_then(|mut l| l.take()) {
            self.en_lecture = false;
            self.visage = match lu {
                Ok((image, detoure, cadre)) => Some(Visage {
                    texture: ctx.load_texture("portrait-d-eve", image, egui::TextureOptions::LINEAR),
                    fond: detoure.then(|| fond_de_studio(ctx)),
                    cadre,
                }),
                Err(e) => {
                    crate::journal::ecrire(&format!("le portrait d'Eve ne se lit pas : {e}"));
                    None
                }
            };
        }
        if !self.en_lecture && maintenant - self.regarde_a >= 2.0 {
            self.regarde_a = maintenant;
            let trouve = fichier(&self.local).and_then(|c| {
                let m = std::fs::metadata(&c).ok()?;
                Some((c, m.modified().ok()?, m.len()))
            });
            if trouve != self.vu {
                self.vu = trouve.clone();
                match trouve {
                    None => self.visage = None,
                    Some((chemin, _, _)) => {
                        self.en_lecture = true;
                        let (lu, ctx) = (self.lu.clone(), ctx.clone());
                        std::thread::spawn(move || {
                            let image = crate::cartes::image_du_disque(&chemin, COTE).map(|image| {
                                let (detoure, cadre) = mise_en_place(&image);
                                (image, detoure, cadre)
                            });
                            if let Ok(mut l) = lu.lock() {
                                *l = Some(image);
                            }
                            ctx.request_repaint();
                        });
                    }
                }
            }
        }
        self.visage.clone()
    }

    /// Le dossier local, où vit le portrait.
    pub(crate) fn local(&self) -> &Path {
        &self.local
    }

    /// Retire le portrait : l'orbe revient tout de suite, sans attendre le prochain regard sur le fichier.
    pub(crate) fn retirer(&mut self) {
        retirer(&self.local);
        (self.vu, self.visage) = (None, None);
    }
}

impl Visage {
    /// Le portrait en grand (la bannière, la tête de la conversation seule), dans le disque de centre `centre` et de
    /// rayon `rayon` : une photo, en disque ; détouré, son fond de studio, puis la personne, que le bas du cercle coupe
    /// et dont la tête sort par le haut.
    pub(crate) fn en_grand(&self, p: &egui::Painter, centre: Pos2, rayon: f32) {
        let Some(fond) = &self.fond else {
            return disque(p, centre, rayon, self.texture.id(), self.cadre);
        };
        disque(p, centre, rayon, fond.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        let (carre, cadre) = (Rect::from_center_size(centre, vec2(2.0 * rayon, 2.0 * rayon)), self.cadre);
        // Un point de l'écran, et sa coordonnée de texture : le carré du disque est le cadre de l'image.
        let vers_uv = move |q: Pos2| {
            pos2(
                cadre.min.x + (q.x - carre.min.x) / carre.width() * cadre.width(),
                cadre.min.y + (q.y - carre.min.y) / carre.height() * cadre.height(),
            )
        };
        // Le haut de l'image, à l'écran : la tête s'arrête là.
        let haut = carre.min.y - cadre.min.y / cadre.height() * carre.height();
        medaillon(p, centre, rayon, haut, self.texture.id(), vers_uv);
    }

    /// Le petit disque (devant les bulles d'Eve, dans l'en-tête du dock et les réglages) : le visage, de plus près ;
    /// détouré, sur son fond de studio.
    pub(crate) fn vignette(&self, p: &egui::Painter, centre: Pos2, rayon: f32) {
        if let Some(fond) = &self.fond {
            disque(p, centre, rayon, fond.id(), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        }
        disque(p, centre, rayon, self.texture.id(), de_pres(self.cadre));
    }
}

/// Le fichier du portrait dans le dossier local : le premier de [`NOMS`] qui existe.
fn fichier(local: &Path) -> Option<PathBuf> {
    NOMS.iter().map(|n| local.join(n)).find(|c| c.is_file())
}

/// Pose `choisie` comme portrait d'Eve : une image que Windows sait lire, copiée dans le dossier local sous son nom de
/// portrait (l'image d'origine peut ensuite bouger) ; un portrait d'avant, d'une autre extension, s'en va.
pub(crate) fn poser(choisie: &Path, local: &Path) -> Result<PathBuf, String> {
    crate::cartes::image_du_disque(choisie, 64)
        .map_err(|_| "ce n'est pas une image que Windows sait lire".to_string())?;
    let ext = choisie.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let nom = NOMS.iter().find(|n| n.ends_with(&format!(".{ext}"))).copied().unwrap_or("portrait.png");
    retirer(local);
    let vers = local.join(nom);
    std::fs::copy(choisie, &vers).map_err(|e| e.to_string())?;
    crate::journal::ecrire(&format!("portrait d'Eve posé : {} (de {})", vers.display(), choisie.display()));
    Ok(vers)
}

/// Retire le portrait d'Eve du dossier local, sous tous ses noms.
fn retirer(local: &Path) {
    for n in NOMS {
        let _ = std::fs::remove_file(local.join(n));
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

/// Une texture peinte en disque, centré sur `centre`, de rayon `rayon` : sa part `uv` (son bord adouci par egui).
fn disque(p: &egui::Painter, centre: Pos2, rayon: f32, texture: egui::TextureId, uv: Rect) {
    let carre = Rect::from_center_size(centre, vec2(2.0 * rayon, 2.0 * rayon));
    p.add(egui::epaint::RectShape::filled(carre, rayon, Color32::WHITE).with_texture(texture, uv));
}

/// La personne détourée, en grand : un seul maillage texturé — le carré du disque, du haut de l'image (`haut`, à
/// l'écran) jusqu'au milieu du cercle, d'où la tête peut sortir, puis la moitié basse du cercle —, son contour adouci
/// sur un point, d'une frange qui s'efface (egui n'adoucit pas un maillage) ; `vers_uv` : la coordonnée de texture d'un
/// point de l'écran.
fn medaillon(
    p: &egui::Painter,
    centre: Pos2,
    rayon: f32,
    haut: f32,
    texture: egui::TextureId,
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
    m.vertices.push(egui::epaint::Vertex { pos: milieu, uv: vers_uv(milieu), color: Color32::WHITE });
    for (i, &point) in contour.iter().enumerate() {
        // Dehors : la perpendiculaire au contour, à gauche de sa marche (l'écran a son y vers le bas).
        let marche = (contour[(i + 1) % k] - contour[(i + k - 1) % k]).normalized();
        let dehors = point + vec2(marche.y, -marche.x);
        for (q, couleur) in [(point, Color32::WHITE), (dehors, Color32::TRANSPARENT)] {
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
        // Le dossier sans portrait : rien ; un portrait posé puis retiré.
        let local = std::env::temp_dir().join(format!("eve-essai-portrait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        std::fs::create_dir_all(&local).unwrap();
        assert_eq!(fichier(&local), None);
        std::fs::write(local.join("portrait.jpg"), b"x").unwrap();
        assert_eq!(fichier(&local), Some(local.join("portrait.jpg")));
        retirer(&local);
        assert_eq!(fichier(&local), None);
        let _ = std::fs::remove_dir_all(&local);
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
