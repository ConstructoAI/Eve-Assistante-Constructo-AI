//! La netteté du portrait d'Eve : ses textures refaites à la taille où il se montre. La carte graphique réduit une image
//! par ses mipmaps — des moyennes de deux sur deux, mêlées d'un étage à l'autre — : un portrait montré au tiers de sa
//! taille, dans la bannière, y perdait plus de la moitié de ses détails (mesuré sur une capture : l'énergie des bords du
//! visage, 619 à l'écran pour 1427 dans une réduction de Lanczos de la même photo). Quand la taille du portrait à l'écran
//! ne bouge plus, chaque texture — la photo, chaque calque — est refaite à cette taille par un filtre de Lanczos, dans un
//! fil, toutes sur la même grille (aucune ne glisse d'un demi-pixel sur une autre) ; le rendu la prend tant que la taille
//! tient ([`Jeu::pour`]), et sinon les textures entières et leurs mipmaps (pendant qu'elle change, par exemple).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Rect, pos2};

use super::Visage;

/// Le temps que la taille du portrait doit tenir avant que ses textures soient refaites, en secondes.
const STABLE: f64 = 0.4;
/// L'écart de taille qu'un jeu de textures nettes supporte encore (une part) : au-delà, il se refait.
const TOLERANCE: f32 = 0.08;
/// Au-dessus de cette échelle, la photo se montre presque à sa taille : ses mipmaps ne l'adoucissent pas.
const ECHELLE_UTILE: f32 = 0.92;

/// La taille où le rendu montre le portrait en grand (le rayon de son disque, en pixels de l'écran), qu'il note à chaque
/// image ([`Visage::en_grand`]) et que le portrait suit : partagée entre les copies du visage.
#[derive(Default)]
pub(super) struct Demande(AtomicU32);

impl Demande {
    pub(super) fn noter(&self, rayon: f32) {
        self.0.store(rayon.to_bits(), Ordering::Relaxed);
    }

    fn lire(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// Un jeu de textures nettes : le rayon du disque (en pixels de l'écran) pour lequel il a été fait ; pour chaque texture
/// entière, la nette et la part de l'image qu'elle couvre (en coordonnées de texture de l'image entière).
pub(super) struct Jeu {
    rayon: f32,
    textures: HashMap<egui::TextureId, (egui::TextureHandle, Rect)>,
}

impl Jeu {
    /// Le jeu, s'il convient à un disque de rayon `rayon` (en pixels de l'écran).
    pub(super) fn pour(jeu: Option<&Arc<Jeu>>, rayon: f32) -> Option<&Jeu> {
        jeu.map(Arc::as_ref).filter(|j| (j.rayon / rayon - 1.0).abs() < TOLERANCE)
    }

    /// La texture nette d'une texture entière, et la part de l'image qu'elle couvre.
    pub(super) fn texture(&self, entiere: egui::TextureId) -> Option<(egui::TextureId, Rect)> {
        self.textures.get(&entiere).map(|(t, uv)| (t.id(), *uv))
    }
}

/// Ce qu'un fil a refait : pour quel visage (l'identifiant de sa photo) et quel rayon, et chaque texture réduite (la
/// texture entière qu'elle remplace, ses pixels, sa part de l'image).
struct Refait {
    photo: egui::TextureId,
    rayon: f32,
    images: Vec<(egui::TextureId, egui::ColorImage, Rect)>,
}

/// Le suivi de la netteté, que tient le portrait : la taille demandée au dernier regard, depuis quand elle tient, et ce
/// qu'un fil est en train de refaire.
#[derive(Default)]
pub(super) struct Suivi {
    vue: f32,
    depuis: f64,
    en_cours: Option<Arc<Mutex<Option<Refait>>>>,
}

impl Suivi {
    /// À chaque image : un jeu refait est chargé dans la carte graphique et posé sur le `visage` ; une taille qui tient
    /// depuis [`STABLE`] secondes, et que le jeu du visage ne couvre pas, en fait refaire un, dans un fil.
    pub(super) fn suivre(&mut self, ctx: &egui::Context, visage: &mut Visage, maintenant: f64) {
        if let Some(fait) = self.en_cours.as_ref().and_then(|e| e.lock().ok().and_then(|mut f| f.take())) {
            self.en_cours = None;
            if fait.photo == visage.texture.id() {
                let textures = fait
                    .images
                    .into_iter()
                    .enumerate()
                    .map(|(k, (entiere, image, uv))| {
                        (entiere, (ctx.load_texture(format!("portrait-net-{k}"), image, super::LISSE), uv))
                    })
                    .collect();
                visage.net = Some(Arc::new(Jeu { rayon: fait.rayon, textures }));
            }
        }
        let rayon = visage.demande.lire();
        if (rayon - self.vue).abs() > 0.03 * self.vue.max(1.0) {
            (self.vue, self.depuis) = (rayon, maintenant);
        }
        let echelle = rayon / visage.rayon_dans_l_image();
        let couvert = Jeu::pour(visage.net.as_ref(), rayon).is_some();
        if self.en_cours.is_some()
            || couvert
            || maintenant - self.depuis < STABLE
            || !(0.05..ECHELLE_UTILE).contains(&echelle)
        {
            return;
        }
        let resultat = Arc::new(Mutex::new(None));
        self.en_cours = Some(resultat.clone());
        let photo = visage.texture.id();
        let taille = visage.taille;
        let sources = visage.sources();
        let ctx = ctx.clone();
        // Dans un fil, qui en fait travailler plusieurs (une centaine de textures : une ou deux secondes pour un seul).
        std::thread::spawn(move || {
            let (l, h) = (taille[0] as f32 * echelle, taille[1] as f32 * echelle);
            let images = super::en_parallele(&sources, |(entiere, image, [x0, y0])| {
                let (petite, [gx0, gy0, gx1, gy1]) = reduire(image, [*x0, *y0], echelle);
                let uv = Rect::from_min_max(pos2(gx0 as f32 / l, gy0 as f32 / h), pos2(gx1 as f32 / l, gy1 as f32 / h));
                (*entiere, petite, uv)
            });
            let images = images.into_iter().flatten().collect();
            if let Ok(mut r) = resultat.lock() {
                *r = Some(Refait { photo, rayon, images });
            }
            ctx.request_repaint();
        });
    }
}

impl Visage {
    /// Ses textures nettes couvrent la taille où il s'est montré en grand à la dernière image (l'essai du portrait
    /// l'attend pour sa capture).
    pub(crate) fn est_net(&self) -> bool {
        Jeu::pour(self.net.as_ref(), self.demande.lire()).is_some()
    }
}

/// Le noyau de Lanczos à trois lobes.
fn lanczos(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-5 {
        1.0
    } else if x < 3.0 {
        let p = std::f32::consts::PI * x;
        3.0 * p.sin() * (p / 3.0).sin() / (p * p)
    } else {
        0.0
    }
}

/// Les poids d'une passe de la réduction : pour chacun des `n` pixels réduits, le premier pixel de l'image (de `l`) qu'il
/// prend, et le poids de chacun — un filtre de Lanczos à la taille réduite, centré sur `origine + (k + ½)/échelle − ½`
/// (en pixels de l'image), qui ne prend que les pixels de l'image, ses poids ramenés à un.
fn poids(origine: f32, n: usize, l: usize, echelle: f32) -> Vec<(usize, Vec<f32>)> {
    let portee = 3.0 / echelle;
    (0..n)
        .map(|k| {
            let centre = origine + (k as f32 + 0.5) / echelle - 0.5;
            let debut = (centre - portee).ceil().max(0.0) as usize;
            let fin = ((centre + portee).floor().max(-1.0) as isize).min(l as isize - 1);
            let mut w: Vec<f32> = (debut as isize..=fin).map(|x| lanczos((x as f32 - centre) * echelle)).collect();
            let total: f32 = w.iter().sum();
            if total.abs() > 1e-6 {
                w.iter_mut().for_each(|p| *p /= total);
            }
            (debut, w)
        })
        .collect()
}

/// Une image (aux pixels prémultipliés, posée en `[x0, y0]` dans l'image entière) réduite à l'échelle `echelle`, sur la
/// grille réduite de l'image entière — son pixel X couvre les pixels X/échelle à (X+1)/échelle de l'image entière — ; et
/// la part qu'elle couvre sur cette grille (gauche, haut, droite, bas).
fn reduire(image: &egui::ColorImage, [x0, y0]: [usize; 2], echelle: f32) -> (egui::ColorImage, [usize; 4]) {
    let [l, h] = image.size;
    let (gx0, gy0) = ((x0 as f32 * echelle).floor() as usize, (y0 as f32 * echelle).floor() as usize);
    let gx1 = (((x0 + l) as f32 * echelle).ceil() as usize).max(gx0 + 1);
    let gy1 = (((y0 + h) as f32 * echelle).ceil() as usize).max(gy0 + 1);
    let origine = [gx0 as f32 / echelle - x0 as f32, gy0 as f32 / echelle - y0 as f32];
    let petite = reechantillonner(image, origine, echelle, [gx1 - gx0, gy1 - gy0]);
    (petite, [gx0, gy0, gx1, gy1])
}

/// Une image aux pixels prémultipliés, rééchantillonnée en `taille` par un filtre de Lanczos à l'échelle `echelle` : le
/// pixel (X, Y) de la sortie prend l'image autour de `origine + (X + ½, Y + ½)/échelle − ½` — les rangées, puis les
/// colonnes. Ses couleurs ne dépassent jamais sa transparence (une image prémultipliée reste juste, même là où le filtre
/// rebondit).
fn reechantillonner(image: &egui::ColorImage, origine: [f32; 2], echelle: f32, taille: [usize; 2]) -> egui::ColorImage {
    let [l, h] = image.size;
    let [lr, hr] = taille;
    let px = poids(origine[0], lr, l, echelle);
    let py = poids(origine[1], hr, h, echelle);
    // Les rangées : chaque rangée de l'image que prennent les colonnes (celles d'une part seulement, pour une
    // vignette), réduite en largeur.
    let haut = py.first().map_or(0, |(debut, _)| *debut).min(h);
    let bas = py.iter().map(|(debut, w)| debut + w.len()).max().unwrap_or(0).clamp(haut, h);
    let mut rangees = vec![[0.0f32; 4]; lr * (bas - haut)];
    let mut rangee = vec![[0.0f32; 4]; l];
    for y in haut..bas {
        for (r, c) in rangee.iter_mut().zip(&image.pixels[y * l..y * l + l]) {
            *r = c.to_array().map(f32::from);
        }
        for (x, (debut, w)) in px.iter().enumerate() {
            let mut s = [0.0f32; 4];
            for (k, &p) in w.iter().enumerate() {
                let c = rangee[debut + k];
                for i in 0..4 {
                    s[i] += p * c[i];
                }
            }
            rangees[(y - haut) * lr + x] = s;
        }
    }
    // Les colonnes : chaque colonne réduite en hauteur.
    let mut pixels = Vec::with_capacity(lr * hr);
    for (debut, w) in &py {
        for x in 0..lr {
            let mut s = [0.0f32; 4];
            for (k, &p) in w.iter().enumerate() {
                let c = rangees[(debut + k - haut) * lr + x];
                for i in 0..4 {
                    s[i] += p * c[i];
                }
            }
            let a = s[3].clamp(0.0, 255.0);
            let c = |v: f32| v.clamp(0.0, a).round() as u8;
            pixels.push(Color32::from_rgba_premultiplied(c(s[0]), c(s[1]), c(s[2]), a.round() as u8));
        }
    }
    egui::ColorImage::new([lr, hr], pixels)
}

/// Les vignettes nettes du portrait (devant les bulles d'Eve, dans l'en-tête du dock et les réglages) : le visage de
/// plus près, réduit une fois à la taille de son petit disque — par ses mipmaps, ses yeux n'y étaient que deux taches —,
/// gardé par rayon (en pixels de l'écran).
#[derive(Default)]
pub(super) struct Vignettes(Mutex<Vec<(u32, egui::TextureHandle)>>);

impl Vignettes {
    /// La vignette d'un disque de rayon `rayon` (en pixels de l'écran) : la part `uv` de l'image (en coordonnées de
    /// texture), réduite au carré du disque ; faite à la première demande.
    pub(super) fn texture(
        &self,
        ctx: &egui::Context,
        image: &egui::ColorImage,
        uv: Rect,
        rayon: f32,
    ) -> Option<egui::TextureId> {
        let cle = rayon.round().max(1.0) as u32;
        let mut faites = self.0.lock().ok()?;
        if let Some((_, t)) = faites.iter().find(|(r, _)| *r == cle) {
            return Some(t.id());
        }
        let [l, h] = image.size;
        let cote = 2 * cle as usize;
        let echelle = cote as f32 / (uv.width() * l as f32);
        let origine = [uv.min.x * l as f32, uv.min.y * h as f32];
        let petite = reechantillonner(image, origine, echelle, [cote, cote]);
        let t = ctx.load_texture(format!("portrait-vignette-{cle}"), petite, super::LISSE);
        let id = t.id();
        faites.push((cle, t));
        Some(id)
    }
}

impl Visage {
    /// Le rayon du disque de la grande vue, en pixels de l'image.
    fn rayon_dans_l_image(&self) -> f32 {
        self.cadre.width() * self.taille[0] as f32 / 2.0
    }

    /// Chaque texture entière à refaire : la photo, puis chaque calque — leur identifiant, leurs pixels, et où ils sont
    /// dans l'image entière (en pixels).
    fn sources(&self) -> Vec<(egui::TextureId, Arc<egui::ColorImage>, [usize; 2])> {
        let [l, h] = self.taille;
        let place = |uv: Rect| [(uv.min.x * l as f32).round() as usize, (uv.min.y * h as f32).round() as usize];
        let calques = self.calques.iter().flatten().chain(
            self.expressions.iter().flat_map(|x| std::iter::once(&x.visage).chain(&x.bouches).chain(&x.levres_closes)),
        );
        std::iter::once((self.texture.id(), self.image.clone(), [0, 0]))
            .chain(calques.map(|c| (c.texture.id(), c.image.clone(), place(c.uv))))
            .collect()
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn une_image_reduite_garde_sa_place_et_sa_lumiere() {
        // Une image uniforme se réduit en une image uniforme, sans que ses bords ne s'assombrissent.
        let gris = egui::ColorImage::new([40, 30], vec![Color32::from_rgb(120, 130, 140); 40 * 30]);
        let (r, part) = reduire(&gris, [0, 0], 0.5);
        assert_eq!((r.size, part), ([20, 15], [0, 0, 20, 15]));
        assert!(r.pixels.iter().all(|&c| c == Color32::from_rgb(120, 130, 140)), "{:?}", &r.pixels[..3]);
        // Un calque posé en (20, 10) dans l'image entière : sur la grille réduite, à (10, 5) ; un point blanc au milieu
        // reste au même endroit de l'image entière.
        let mut c = egui::ColorImage::new([16, 16], vec![Color32::TRANSPARENT; 256]);
        c.pixels[8 * 16 + 8] = Color32::WHITE;
        let (r, [gx0, gy0, gx1, gy1]) = reduire(&c, [20, 10], 0.5);
        assert_eq!([gx0, gy0, gx1, gy1], [10, 5, 18, 13]);
        let (k, plus) = r.pixels.iter().enumerate().max_by_key(|(_, c)| c.a()).unwrap();
        assert!(plus.a() > 0 && plus.r() <= plus.a(), "{plus:?}");
        let (x, y) = (gx0 + k % r.size[0], gy0 + k / r.size[0]);
        // Le pixel blanc (28,5 ; 18,5 au centre) tombe dans le pixel réduit (14, 9).
        assert_eq!((x, y), (14, 9));
        // Une image prémultipliée reste juste : jamais plus de couleur que d'opacité, même là où le filtre rebondit.
        let mut bord = egui::ColorImage::new([12, 12], vec![Color32::TRANSPARENT; 144]);
        for y in 0..12 {
            for x in 6..12 {
                bord.pixels[y * 12 + x] = Color32::WHITE;
            }
        }
        let (r, _) = reduire(&bord, [0, 0], 0.4);
        assert!(r.pixels.iter().all(|c| c.r() <= c.a() && c.g() <= c.a() && c.b() <= c.a()));
        // Un jeu ne sert qu'à la taille pour laquelle il a été fait, à quelques pour cent près.
        let jeu = Arc::new(Jeu { rayon: 100.0, textures: HashMap::new() });
        assert!(Jeu::pour(Some(&jeu), 104.0).is_some() && Jeu::pour(Some(&jeu), 120.0).is_none());
        assert!(Jeu::pour(None, 100.0).is_none());
    }

    #[test]
    fn le_jeu_net_se_fait_quand_la_taille_tient() {
        // Un visage de 64 sur 48 (son disque : 24 pixels de rayon dans l'image), et un calque de 20 sur 12 posé en
        // (16, 8).
        let ctx = egui::Context::default();
        let lisse = super::super::LISSE;
        let photo = Arc::new(egui::ColorImage::new([64, 48], vec![Color32::from_rgb(200, 150, 120); 64 * 48]));
        let pixels = Arc::new(egui::ColorImage::new([20, 12], vec![Color32::from_rgb(90, 60, 40); 20 * 12]));
        let uv = Rect::from_min_max(pos2(16.0 / 64.0, 8.0 / 48.0), pos2(36.0 / 64.0, 20.0 / 48.0));
        let calque = super::super::Calque {
            texture: ctx.load_texture("calque", pixels.clone(), lisse),
            image: pixels,
            uv,
            geometrie: Arc::default(),
        };
        let mut visage = Visage {
            texture: ctx.load_texture("photo", photo.clone(), lisse),
            image: photo,
            taille: [64, 48],
            calques: [vec![calque.clone()], vec![], vec![], vec![], vec![], vec![], vec![]],
            expressions: Vec::new(),
            fond: None,
            cadre: Rect::from_min_max(pos2(0.0, 0.0), pos2(0.75, 1.0)),
            demande: Arc::default(),
            net: None,
            vignettes: Arc::default(),
        };
        // Montré à 12 pixels de rayon : rien tant que la taille n'a pas tenu, puis un fil refait les textures, au demi.
        let mut suivi = Suivi::default();
        visage.demande.noter(12.0);
        suivi.suivre(&ctx, &mut visage, 10.0);
        suivi.suivre(&ctx, &mut visage, 10.2);
        assert!(suivi.en_cours.is_none());
        suivi.suivre(&ctx, &mut visage, 10.5);
        assert!(suivi.en_cours.is_some());
        for _ in 0..500 {
            if visage.net.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            suivi.suivre(&ctx, &mut visage, 10.6);
        }
        assert!(visage.est_net(), "le jeu net est posé");
        let jeu = visage.net.clone().unwrap();
        // La photo, réduite de moitié, couvre toute l'image ; le calque, la même part qu'avant.
        let (_, uv_photo) = jeu.texture(visage.texture.id()).unwrap();
        assert_eq!(uv_photo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        let (_, uv_calque) = jeu.texture(calque.texture.id()).unwrap();
        assert!((uv_calque.min - uv.min).length() < 1e-6 && (uv_calque.max - uv.max).length() < 1e-6, "{uv_calque:?}");
        // Montré plus grand d'un tiers : le jeu ne convient plus.
        visage.demande.noter(16.0);
        assert!(!visage.est_net());
    }
}
