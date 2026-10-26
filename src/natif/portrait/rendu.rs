//! Le rendu du portrait d'Eve : en grand (la bannière, la tête de la conversation seule) — la photo, puis les couches de
//! sa pose, une photo en disque ; détouré, sur son fond de studio, la tête hors du cercle, qui bouge, les épaules en
//! place — et en petit (les bulles, le dock, les réglages), le visage de plus près.
//!
//! En grand, tout se peint sur un même treillis ([`Treillis`]) : un nœud tous les [`PAS`] pixels de l'image, chacun
//! déplacé, puis chaque couche peinte sur lui. Deux mouvements le déplacent. Le passage d'une pose à l'autre, d'abord :
//! chaque calque connaît où chaque point de la photo se retrouve dans sa pose ([`Geometrie`], son flot optique) ; le
//! treillis prend la forme du mélange du moment — chaque calque selon sa part visible — et chaque couche s'y peint en
//! allant chercher dans sa pose le point qui y vient. Les couches tombent l'une sur l'autre : à mi-chemin d'un
//! clignement, une seule paupière, à mi-hauteur, et non deux, l'une sur l'autre ; un sourcil qui monte, une mâchoire qui
//! descend. Puis la tête ([`Mouvement`]) : tournée et hochée en trois dimensions — une tête n'est pas une carte :
//! chaque point de son visage va d'autant plus loin qu'il est en avant de l'axe de la tête ([`Rangee`]) —, puis
//! penchée autour de la base du cou, elle entraîne tout ce qui est au-dessus du menton, le cou et les cheveux de moins
//! en moins, et rien des épaules ni du bord du disque ([`poids_de_la_tete`]). Puis le corps
//! ([`deplacement_du_corps`]) : son souffle soulève la poitrine et les épaules, la tête portée par elles. Puis les
//! longueurs des cheveux, qui traînent un peu derrière la tête quand elle bouge ([`super::cheveux`]). Le bord du
//! disque ne bouge jamais, et les épaules le touchent : loin de lui, le corps déplace le treillis ; près de lui, il
//! glisse dessous, comme derrière le cadre d'un médaillon — chaque couche va chercher dans son image le point qui vient
//! là ([`BORD`]). Un seul maillage par couche, du haut de l'image au bas du disque : aucune couture.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};

use super::flot::{self, Gris};
use super::{Corps, Couche, Pose, Tete, Visage, de_pres, nettete};
use crate::natif::theme::erp;

/// L'écart des nœuds du treillis, en pixels de l'image : le flot est lissé sur six pixels, et la tête se plie sur des
/// dizaines ; huit suffisent, et le disque garde un bord rond.
pub(super) const PAS: usize = 8;
/// Le pivot de la tête, en parts du rayon depuis le centre du disque (y vers le bas) : la base du cou, où une tête se
/// penche.
pub(super) const PIVOT: Vec2 = vec2(0.0, 0.15);
/// Le bas du menton, en parts du rayon depuis le centre du disque : au-dessus, tout suit la tête d'un bloc.
const MENTON: f32 = -0.12;
/// Ce que la tête entraîne encore sous le menton, en parts du rayon : le cou et les cheveux, jusqu'à la base du cou ; les
/// épaules, presque rien.
const PORTEE_DU_COU: f32 = 0.30;
const PORTEE_DES_EPAULES: f32 = 0.10;
/// Ce que le souffle soulève le haut du corps par unité d'air ([`Corps`] : 1, une inspiration au repos), en part du
/// rayon — un pixel et demi au film du portrait ; la part qu'en prend la tête, portée par lui ; ce dont la poitrine
/// s'élargit, en part de sa demi-largeur.
const SOUFFLE: f32 = 0.006;
const SOUFFLE_DE_LA_TETE: f32 = 0.5;
const ELARGI_PAR_LE_SOUFFLE: f32 = 0.0025;
/// Une photo, dont on ne sait pas où sont les épaules : son souffle la grandit depuis le bas du disque, de cette part
/// par unité d'air.
const SOUFFLE_DE_LA_PHOTO: f32 = 0.003;
/// Les épaules, de part et d'autre du cou (en part du rayon depuis le milieu du disque) : à partir d'ici, elles se
/// lèvent de plus en plus, et tout entières à partir de là — le cou reste en place, les cheveux qui tombent sur elles
/// les suivent à mesure.
const COU: f32 = 0.2;
const EPAULES: f32 = 0.7;
/// Le pivot du torse quand il se penche du côté de la tête (en part du rayon depuis le centre du disque) : le bas de la
/// poitrine, au bas du disque — le haut du corps se plie au-dessus.
const PIVOT_DU_TORSE: Vec2 = vec2(0.0, 1.0);
/// Ce qui grandit quand elle se penche vers la personne, comme ce qui s'approche d'un objectif : autour de ses yeux.
const YEUX: Vec2 = vec2(0.0, -0.55);
/// La bande, le long du bord de ce qui se peint ([`distance_au_bord`], en part du rayon), où le corps glisse sous le
/// bord au lieu d'y déplacer le treillis : le bord ne bouge pas, et rien ne le dépasse.
const BORD: f32 = 0.2;

/// La géométrie d'un calque, aux nœuds du treillis qu'il couvre (les colonnes `i0` à `i0 + ni`, les rangées `j0` à
/// `j0 + nj`, en nœuds de l'image) : où chaque point de la photo se retrouve dans sa pose (son flot, en pixels de
/// l'image), et la part de lui qui se voit (son masque).
#[derive(Default)]
pub(super) struct Geometrie {
    i0: usize,
    j0: usize,
    ni: usize,
    nj: usize,
    noeuds: Vec<Noeud>,
}

/// Un nœud d'un calque : le déplacement de la photo vers sa pose (en pixels de l'image), et la part qui se voit.
#[derive(Clone, Copy, Default)]
struct Noeud {
    dx: f32,
    dy: f32,
    masque: f32,
}

impl Geometrie {
    /// Le nœud (i, j) de l'image, s'il est dans ce calque.
    fn en(&self, i: usize, j: usize) -> Option<Noeud> {
        let (di, dj) = (i.checked_sub(self.i0)?, j.checked_sub(self.j0)?);
        (di < self.ni && dj < self.nj).then(|| self.noeuds[dj * self.ni + di])
    }
}

/// La géométrie d'un calque, dans le fil de lecture : le flot de la `photo` vers la `pose` ([`flot::flot`], sur la part
/// `[x0, y0, x1, y1]` de l'image où se voit le calque, en pixels), et son `masque` (pour toute l'image), pris aux nœuds
/// du treillis de cette part. Rien ne se voit (une part vide) : aucune.
pub(super) fn geometrie(
    photo: &egui::ColorImage,
    pose: &egui::ColorImage,
    masque: &[f32],
    [x0, y0, x1, y1]: [usize; 4],
) -> Geometrie {
    let l = photo.size[0];
    let (i0, j0) = (x0.div_ceil(PAS), y0.div_ceil(PAS));
    let (ni, nj) =
        ((x1.saturating_sub(1) / PAS + 1).saturating_sub(i0), (y1.saturating_sub(1) / PAS + 1).saturating_sub(j0));
    if ni < 2 || nj < 2 || x1 - x0 < 8 || y1 - y0 < 8 {
        return Geometrie::default();
    }
    let gris = |image: &egui::ColorImage| Gris {
        l: x1 - x0,
        h: y1 - y0,
        v: (y0..y1)
            .flat_map(|y| image.pixels[y * l + x0..y * l + x1].iter())
            .map(|c| 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b()))
            .collect(),
    };
    let champ = flot::flot(&gris(photo), &gris(pose));
    let noeuds = (0..nj)
        .flat_map(|dj| (0..ni).map(move |di| (i0 + di, j0 + dj)))
        .map(|(i, j)| {
            let (x, y) = (i * PAS, j * PAS);
            let [dx, dy] = champ.en((x - x0) as f32, (y - y0) as f32);
            Noeud { dx, dy, masque: masque[y * l + x] }
        })
        .collect();
    Geometrie { i0, j0, ni, nj, noeuds }
}

impl Visage {
    /// Le portrait en grand (la bannière, la tête de la conversation seule), dans le disque de centre `centre` et de
    /// rayon `rayon`, dans sa pose du moment ([`super::Portrait::pose_du_moment`]) : la photo, puis chacune de ses
    /// couches, sur le treillis du moment ([`Treillis`]) — une photo, en disque ; détouré, sur son fond de studio, la
    /// personne que le bas du cercle coupe et dont la tête sort par le haut, et qui bouge, sa poitrine et ses épaules
    /// avec son souffle. Une photo respire autrement : elle grandit un peu depuis le bas du disque, comme une poitrine
    /// qui s'emplit.
    pub(crate) fn en_grand(&self, p: &egui::Painter, centre: Pos2, rayon: f32, pose: &Pose) {
        let k = if self.fond.is_some() { 1.0 } else { 1.0 + SOUFFLE_DE_LA_PHOTO * pose.corps.souffle.max(0.0) };
        let cadre = Rect::from_center_size(
            pos2(self.cadre.center().x, self.cadre.max.y - self.cadre.height() / (2.0 * k)),
            self.cadre.size() / k,
        );
        // Le fond, un point plus petit que le disque : la frange du portrait passe seule au bord (deux franges l'une sur
        // l'autre y laissaient un filet clair, sous le bord sombre de la veste).
        if let Some(fond) = &self.fond {
            rond(p, centre, rayon - 1.0, fond.id(), Color32::WHITE);
        }
        let t = Treillis::nouveau(self.taille, cadre, centre, rayon, self.fond.is_some(), pose);
        let photo = Couche {
            texture: self.texture.id(),
            uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            part: 1.0,
            geometrie: None,
        };
        // Les textures refaites à la taille du disque, si elles y sont ([`nettete`]) ; sinon, les entières.
        let rayon_ecran = rayon * p.pixels_per_point();
        self.demande.noter(rayon_ecran);
        let net = nettete::Jeu::pour(self.net.as_ref(), rayon_ecran);
        for c in std::iter::once(&photo).chain(pose.couches()) {
            let nette = net.and_then(|j| j.texture(c.texture)).map(|(texture, uv)| Couche { texture, uv, ..c.clone() });
            if let Some(m) = t.maillage(nette.as_ref().unwrap_or(c)) {
                p.add(egui::Shape::mesh(m));
            }
        }
    }

    /// Le petit disque (devant les bulles d'Eve, dans l'en-tête du dock et les réglages) : le visage, de plus près,
    /// réduit à la taille du disque ([`nettete::Vignettes`]) ; détouré, sur son fond de studio.
    pub(crate) fn vignette(&self, p: &egui::Painter, centre: Pos2, rayon: f32) {
        let entiere = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        if let Some(fond) = &self.fond {
            disque(p, centre, rayon, fond.id(), entiere, Color32::WHITE);
        }
        let visage = de_pres(self.cadre);
        match self.vignettes.texture(p.ctx(), &self.image, visage, rayon * p.pixels_per_point()) {
            Some(nette) => disque(p, centre, rayon, nette, entiere, Color32::WHITE),
            None => disque(p, centre, rayon, self.texture.id(), visage, Color32::WHITE),
        }
    }
}

/// Le fond du disque d'un portrait détouré : la lumière d'un studio, claire derrière la tête, qui descend au bleu pâle de
/// l'ERP sur les bords.
pub(super) fn fond_de_studio(ctx: &egui::Context) -> egui::TextureHandle {
    const N: usize = 64;
    let clair = Color32::from_rgb(0xF7, 0xFA, 0xFF);
    let pixels = (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32 / (N - 1) as f32, (i / N) as f32 / (N - 1) as f32);
            crate::natif::melange(clair, erp::FOND_1, (vec2(x - 0.5, y - 0.38).length() / 0.7).clamp(0.0, 1.0))
        })
        .collect();
    ctx.load_texture("fond-du-portrait", egui::ColorImage::new([N, N], pixels), egui::TextureOptions::LINEAR)
}

/// Une texture peinte en disque, centré sur `centre`, de rayon `rayon` : sa part `uv` (son bord adouci par egui),
/// multipliée par `teinte` (blanc : telle quelle ; un blanc à moitié transparent : à moitié fondue). Pour un petit disque :
/// egui n'arrondit les coins d'un rectangle que de 255 points au plus.
fn disque(p: &egui::Painter, centre: Pos2, rayon: f32, texture: egui::TextureId, uv: Rect, teinte: Color32) {
    let carre = Rect::from_center_size(centre, vec2(2.0 * rayon, 2.0 * rayon));
    p.add(egui::epaint::RectShape::filled(carre, rayon, teinte).with_texture(texture, uv));
}

/// Une texture entière peinte en disque par un maillage, de toute taille (un grand [`disque`] devenait un carré aux
/// coins ronds) : un éventail, son bord adouci d'une frange d'un point vers le dehors, comme celle du treillis.
fn rond(p: &egui::Painter, centre: Pos2, rayon: f32, texture: egui::TextureId, teinte: Color32) {
    let n = ((rayon * 0.6) as u32).clamp(48, 160);
    let prise = |d: Vec2| pos2((d.x + 1.0) / 2.0, (d.y + 1.0) / 2.0);
    let mut m = egui::Mesh::with_texture(texture);
    m.vertices.push(egui::epaint::Vertex { pos: centre, uv: prise(Vec2::ZERO), color: teinte });
    for k in 0..n {
        let d = Vec2::angled(std::f32::consts::TAU * k as f32 / n as f32);
        m.vertices.push(egui::epaint::Vertex { pos: centre + d * rayon, uv: prise(d), color: teinte });
        m.vertices.push(egui::epaint::Vertex {
            pos: centre + d * (rayon + 1.0),
            uv: prise(d),
            color: Color32::TRANSPARENT,
        });
    }
    for k in 0..n {
        let (a, b) = (1 + 2 * k, 1 + 2 * ((k + 1) % n));
        m.indices.extend([0, a, b, a, a + 1, b, a + 1, b + 1, b]);
    }
    p.add(egui::Shape::mesh(m));
}

/// Un passage adouci, de 0 à 1 : lent au départ et à l'arrivée.
fn lisse(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Ce que la tête entraîne du point (u, v) du portrait (en parts du rayon depuis le centre du disque, v vers le bas), de
/// 0 à 1 : tout au-dessus du menton, d'un bloc ; sous lui, de moins en moins — le cou et les cheveux jusqu'à la base du
/// cou, les épaules à peine —, et rien sur le bord du disque sous son milieu, qui ne bouge jamais.
fn poids_de_la_tete(u: f32, v: f32) -> f32 {
    let vers_les_epaules = lisse((u.abs() - 0.55) / 0.35);
    let portee = PORTEE_DU_COU + (PORTEE_DES_EPAULES - PORTEE_DU_COU) * vers_les_epaules;
    1.0 - lisse((v - MENTON) / portee)
}

/// La profondeur du portrait — de combien chaque point est en avant de l'axe autour duquel la tête tourne et hoche, le
/// haut du cou entre les oreilles, en part du rayon du disque —, en deux couches, chacune sur le milieu du visage, tous
/// les [`ECART`] de hauteur à partir de [`PREMIER`] (v vers le bas, depuis le centre du disque), avec sa demi-largeur
/// (jusqu'où elle s'arrondit à rien) : la tête et ses cheveux, larges (les cheveux qui encadrent le visage tournent
/// avec lui) ; et le visage devant elle, plus étroit, qui fait le bas du visage plus rond que les cheveux à côté. Et le
/// nez ([`nez`]). Mesurée sur LivePortrait — le chemin de chaque point quand il tourne et hoche la tête d'Eve de six
/// degrés, son flot divisé par l'angle : la tête comme le visage, à un vingtième du rayon près —, sauf sous le menton,
/// où il cloue l'image (le menton à 9 cm en avant de l'axe, le cou à 5 : un centimètre fait un vingtième du rayon).
const PREMIER: f32 = -1.3;
const ECART: f32 = 0.1;
const TETE: [f32; 17] =
    [0.0, 0.12, 0.275, 0.39, 0.455, 0.48, 0.49, 0.445, 0.385, 0.345, 0.305, 0.235, 0.19, 0.14, 0.09, 0.055, 0.025];
const LARGEUR_DE_LA_TETE: [f32; 17] =
    [0.25, 0.32, 0.39, 0.45, 0.53, 0.58, 0.645, 0.71, 0.75, 0.74, 0.685, 0.7, 0.76, 0.79, 0.8, 0.8, 0.8];
const VISAGE: [f32; 17] =
    [0.0, 0.0, 0.03, 0.03, 0.0, 0.0, 0.0, 0.0, 0.045, 0.105, 0.13, 0.195, 0.225, 0.195, 0.155, 0.135, 0.115];
const LARGEUR_DU_VISAGE: [f32; 17] =
    [0.58, 0.53, 0.48, 0.435, 0.39, 0.35, 0.32, 0.3, 0.3, 0.305, 0.285, 0.225, 0.255, 0.265, 0.26, 0.255, 0.25];
/// Le milieu du visage d'Eve, en part du rayon (le portrait le place sous le milieu de sa tête : il penche à peine à
/// gauche).
const MILIEU_DU_VISAGE: f32 = -0.015;
/// Le nez : son bout (en part du rayon depuis le centre du disque), sa demi-largeur, ce qu'il couvre au-dessus de son
/// bout (son arête, jusqu'entre les sourcils) et au-dessous (jusqu'à la lèvre), et ce qu'il avance devant le visage.
const NEZ: Vec2 = vec2(-0.025, -0.39);
const NEZ_DEMI_LARGEUR: f32 = 0.135;
const NEZ_AU_DESSUS: f32 = 0.3;
const NEZ_AU_DESSOUS: f32 = 0.08;
const AVANCE_DU_NEZ: f32 = 0.062;

/// La profondeur du portrait à une hauteur `v` (en part du rayon depuis le centre du disque, vers le bas) : de combien
/// chacun de ses points est en avant de l'axe de la tête ([`TETE`], [`VISAGE`], [`nez`]) — le bout du nez, le plus ;
/// rien au-dessus du crâne, ni au bord des cheveux, ni sur les épaules. Ses tables lues une fois pour une rangée du
/// treillis (ses nœuds sont à la même hauteur) : la profondeur de la tête et du visage sur le milieu, l'inverse de
/// leurs demi-largeurs.
#[derive(Clone, Copy)]
pub(super) struct Rangee {
    v: f32,
    tete: f32,
    par_tete: f32,
    visage: f32,
    par_visage: f32,
}

impl Rangee {
    pub(super) fn de(v: f32) -> Rangee {
        let (lire, largeur) =
            (|t: &[f32]| dans_la_table(t, v).max(0.0), |t: &[f32]| 1.0 / dans_la_table(t, v).max(0.05));
        Rangee {
            v,
            tete: lire(&TETE),
            par_tete: largeur(&LARGEUR_DE_LA_TETE),
            visage: lire(&VISAGE),
            par_visage: largeur(&LARGEUR_DU_VISAGE),
        }
    }

    /// La profondeur du point de cette rangée à `u` (en part du rayon depuis le centre du disque) : chaque couche
    /// s'arrondit de son milieu à rien ([`tour`]), et le nez avance.
    pub(super) fn profondeur(&self, u: f32) -> f32 {
        let d = (u - MILIEU_DU_VISAGE).abs();
        self.tete * tour(d * self.par_tete) + self.visage * tour(d * self.par_visage) + nez(vec2(u, self.v))
    }
}

/// Le tour d'une couche de la profondeur, à la part `s` de sa demi-largeur : presque plat au milieu, puis qui fuit de
/// plus en plus vite, comme une tête vue d'en haut, et s'annule au bout sans pli (sa pente aussi).
fn tour(s: f32) -> f32 {
    if s < 1.0 {
        let f = 1.0 - s * s * s;
        f * f
    } else {
        0.0
    }
}

/// Ce que le nez avance au point `p` devant le visage : tout, à son bout ; de moins en moins vers ses ailes, entre les
/// sourcils et vers la lèvre ; rien au-delà, sans pli.
fn nez(p: Vec2) -> f32 {
    let haut = if p.y < NEZ.y { NEZ_AU_DESSUS } else { NEZ_AU_DESSOUS };
    let q = ((p.x - NEZ.x) / NEZ_DEMI_LARGEUR).powi(2) + ((p.y - NEZ.y) / haut).powi(2);
    if q < 1.0 { AVANCE_DU_NEZ * (1.0 - q) * (1.0 - q) } else { 0.0 }
}

/// La valeur d'une table de nœuds réguliers (tous les [`ECART`] à partir de [`PREMIER`]) en `v` : entre deux nœuds, la
/// courbe douce qui passe par eux (Catmull-Rom : sa pente aussi va d'un nœud au suivant sans à-coup) ; au-delà de la
/// table, celle de son bout.
fn dans_la_table(table: &[f32], v: f32) -> f32 {
    let n = table.len();
    let x = ((v - PREMIER) / ECART).clamp(0.0, (n - 1) as f32);
    let k = (x as usize).min(n - 2);
    let t = x - k as f32;
    let p = |i: usize| table[i.min(n - 1)];
    let (p0, p1, p2, p3) = (p(k.saturating_sub(1)), p(k), p(k + 1), p(k + 2));
    let (t2, t3) = (t * t, t * t * t);
    0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

/// La tête à une image, prête à déplacer chaque point du portrait ([`Mouvement::deplacement`]) : ses angles — les
/// sinus de son lacet et de son tangage, le sinus et le cosinus de son roulis, calculés une fois pour tous les nœuds —
/// et son déplacement.
#[derive(Clone, Copy)]
pub(super) struct Mouvement {
    lacet: f32,
    tangage: f32,
    roulis: (f32, f32),
    decale: Vec2,
}

impl Mouvement {
    pub(super) fn de(t: &Tete) -> Mouvement {
        Mouvement { lacet: t.lacet.sin(), tangage: t.tangage.sin(), roulis: t.roulis.sin_cos(), decale: vec2(t.x, t.y) }
    }

    /// Où la tête emmène le point `p` du portrait (en parts du rayon depuis le centre du disque, y vers le bas), qui
    /// est à la profondeur `z` (celle de ce point sur la photo, avant les poses : [`Rangee::profondeur`]), s'il la
    /// suivait tout entier — ce qu'elle en entraîne vraiment est à part ([`poids_de_la_tete`]) : tourné et hoché en
    /// trois dimensions, d'autant plus qu'il est en avant de l'axe de la tête — vu de face, il glisse de côté quand
    /// elle tourne, vers le bas quand elle baisse le menton, et le reste de son chemin, vers nous ou loin de nous, ne
    /// se voit pas (ses angles sont petits) ; puis penché autour de la base du cou ([`PIVOT`]), et déplacé. Son chemin,
    /// en parts du rayon.
    pub(super) fn deplacement(&self, p: Vec2, z: f32) -> Vec2 {
        let e = p + vec2(z * self.lacet, z * self.tangage) - PIVOT;
        let (s, c) = self.roulis;
        vec2(e.x * c - e.y * s, e.x * s + e.y * c) + PIVOT + self.decale - p
    }
}

/// Ce que le corps déplace du point `p` du portrait (en parts du rayon depuis le centre du disque, y vers le bas), que
/// la tête emporte de `tete` ([`poids_de_la_tete`]) : le souffle soulève le haut du corps — la poitrine, les épaules,
/// le cou et les cheveux qui y tombent —, la tête portée par lui à moitié, et élargit un peu la poitrine ; les épaules
/// se lèvent ou tombent, de part et d'autre du cou ([`COU`], [`EPAULES`]) ; le torse se redresse, et se penche du côté
/// de la tête autour du bas de la poitrine ([`PIVOT_DU_TORSE`]) — la tête gardant la place que lui donnent ses gestes ;
/// et quand elle se penche vers la personne, tout grandit autour de ses yeux ([`YEUX`]), la tête comprise.
fn deplacement_du_corps(p: Vec2, tete: f32, corps: &Corps) -> Vec2 {
    let buste = 1.0 - tete;
    let souffle = SOUFFLE * corps.souffle * (SOUFFLE_DE_LA_TETE + (1.0 - SOUFFLE_DE_LA_TETE) * buste);
    let epaules = corps.epaules * lisse((p.x.abs() - COU) / (EPAULES - COU));
    let leve = vec2(ELARGI_PAR_LE_SOUFFLE * corps.souffle * buste * p.x, -souffle - (epaules + corps.monte) * buste);
    let e = p - PIVOT_DU_TORSE;
    leve + vec2(-e.y, e.x) * (corps.roulis * buste) + (p - YEUX) * corps.avance
}

/// La distance du point `p` (en parts du rayon depuis le centre du disque) au bord de ce qui se peint d'un portrait
/// détouré : sous le milieu du disque, son cercle ; au-dessus, ses côtés (la tête sort par le haut).
fn distance_au_bord(p: Vec2) -> f32 {
    (1.0 - if p.y > 0.0 { p.length() } else { p.x.abs() }).max(0.0)
}

/// Le pas de l'arc qui borde le disque entre deux nœuds du bord, en radians (deux degrés) : près du bas, deux rangées
/// du treillis s'écartent sur le cercle, et leur corde se verrait.
const PAS_DE_L_ARC: f32 = 0.035;

/// Le treillis du portrait à une image : les nœuds de l'image qui couvrent le portrait (les colonnes `i0` à `i0 + ni`,
/// les rangées `j0` à `j0 + nj`) ; chacun, ramené dans sa forme — détouré, du haut de l'image jusqu'au milieu du disque,
/// puis le bas du cercle ; une photo, le cercle entier — (`place`, en pixels de l'image ; `libre` : il n'a pas eu à y
/// être ramené), le flot du mélange des calques en lui (`flot`), ce dont le corps glisse sous lui (`glisse`, en pixels
/// de l'image : chaque couche y prend son image d'autant plus loin), et où il se peint (`ecran`) ; les nœuds du bord du
/// cercle, dans l'ordre (`bord`), entre lesquels l'arc se referme, bordé d'une frange qui s'efface (egui n'adoucit pas
/// un maillage) ; la taille de l'image, le disque dans l'image (`c`, `r`, en pixels) et à l'écran ; détouré ou non, et
/// le corps du moment.
struct Treillis {
    i0: usize,
    j0: usize,
    ni: usize,
    nj: usize,
    place: Vec<Pos2>,
    libre: Vec<bool>,
    flot: Vec<Vec2>,
    glisse: Vec<Vec2>,
    ecran: Vec<Pos2>,
    bord: Vec<usize>,
    taille: Vec2,
    c: Pos2,
    r: f32,
    centre: Pos2,
    rayon: f32,
    detoure: bool,
    corps: Corps,
}

impl Treillis {
    /// Le treillis du portrait (`taille` : celle de l'image, en pixels ; `cadre` : le carré du disque, en coordonnées de
    /// texture) peint dans le disque de centre `centre` et de rayon `rayon`, `detoure` ou non, dans la `pose` du moment :
    /// chaque nœud prend la forme du mélange de ses calques, puis la tête et le corps (détouré seulement) l'emportent.
    fn nouveau(taille: [usize; 2], cadre: Rect, centre: Pos2, rayon: f32, detoure: bool, pose: &Pose) -> Treillis {
        let taille = vec2(taille[0] as f32, taille[1] as f32);
        let c = pos2(cadre.center().x * taille.x, cadre.center().y * taille.y);
        let r = cadre.width() * taille.x / 2.0;
        let pas = PAS as f32;
        let haut = if detoure { 0.0 } else { c.y - r };
        let (i0, j0) = (((c.x - r) / pas).floor().max(0.0) as usize, (haut / pas).floor().max(0.0) as usize);
        let (i1, j1) = (((c.x + r) / pas).ceil() as usize, ((c.y + r) / pas).ceil() as usize);
        let (ni, nj) = (i1 + 1 - i0, j1 + 1 - j0);
        let couches: Vec<&Couche> = pose.couches().filter(|c| c.geometrie.is_some()).collect();
        let tete = Mouvement::de(&pose.tete);
        let mut t = Treillis {
            i0,
            j0,
            ni,
            nj,
            place: Vec::with_capacity(ni * nj),
            libre: Vec::with_capacity(ni * nj),
            flot: Vec::with_capacity(ni * nj),
            glisse: Vec::with_capacity(ni * nj),
            ecran: Vec::with_capacity(ni * nj),
            bord: Vec::new(),
            taille,
            c,
            r,
            centre,
            rayon,
            detoure,
            corps: pose.corps,
        };
        for j in j0..=j1 {
            // La profondeur de la tête à la hauteur de cette rangée (tous ses nœuds sont à la même, dans le portrait).
            let rangee = Rangee::de((((j * PAS) as f32).clamp(haut, c.y + r) - c.y) / r);
            for i in i0..=i1 {
                let noeud = pos2((i * PAS) as f32, (j * PAS) as f32);
                // Dans la forme du portrait : la largeur du disque ; sous son milieu (partout, pour une photo), le
                // cercle.
                let mut q = pos2(noeud.x.clamp(c.x - r, c.x + r), noeud.y.clamp(haut, c.y + r));
                if !detoure || q.y > c.y {
                    let demi = (r * r - (q.y - c.y).powi(2)).max(0.0).sqrt();
                    q.x = q.x.clamp(c.x - demi, c.x + demi);
                }
                let libre = q == noeud;
                // Le flot du mélange : chaque calque selon la part de lui qui se voit, sous ceux qui le couvrent.
                let mut f = Vec2::ZERO;
                if libre {
                    let mut reste = 1.0;
                    for couche in couches.iter().rev() {
                        if let Some(n) = couche.geometrie.as_ref().and_then(|g| g.en(i, j)) {
                            let part = couche.part.clamp(0.0, 1.0) * n.masque;
                            f += vec2(n.dx, n.dy) * part * reste;
                            reste *= 1.0 - part;
                        }
                    }
                }
                // À l'écran : en parts du rayon depuis le centre du disque, déplacé par la tête, puis par le corps —
                // loin du bord, il déplace le nœud ; près du bord, il glisse sous lui.
                let depart = (q - c) / r;
                let mut d = (q + f - c) / r;
                let mut glisse = Vec2::ZERO;
                if detoure {
                    let poids = poids_de_la_tete(depart.x, depart.y);
                    if poids > 0.0 {
                        d += tete.deplacement(d, rangee.profondeur(depart.x)) * poids;
                    }
                    let corps = deplacement_du_corps(depart, poids, &pose.corps);
                    let loin = lisse(distance_au_bord(depart) / BORD);
                    d += corps * loin;
                    glisse = corps * ((loin - 1.0) * r);
                    // Les longueurs des cheveux, en retard sur la tête ([`super::cheveux`]) : autant qu'elle emporte
                    // le nœud, rien sur le bord.
                    d += pose.cheveux.ecart(i, j, depart.y) * (poids * loin);
                }
                t.place.push(q);
                t.libre.push(libre);
                t.flot.push(f);
                t.glisse.push(glisse);
                t.ecran.push(centre + d * rayon);
            }
        }
        // Le bord du cercle : le premier nœud de chaque rangée qu'il borne, de haut en bas, puis le dernier, de bas en
        // haut.
        let bornees: Vec<usize> = (0..nj).filter(|&dj| !detoure || t.place[dj * ni].y > c.y).collect();
        let gauche = bornees.iter().map(|&dj| dj * ni);
        let droite = bornees.iter().rev().map(|&dj| dj * ni + ni - 1);
        t.bord = gauche.chain(droite).collect();
        t
    }

    /// La coordonnée de texture, dans l'image entière, de ce qui se voit en un point de l'écran qui ne bouge pas (sur
    /// le bord du disque) : le corps y glisse dessous.
    fn image_de(&self, q: Pos2) -> Pos2 {
        let depart = (q - self.centre) / self.rayon;
        let mut p = self.c + depart * self.r;
        if self.detoure {
            p -= deplacement_du_corps(depart, poids_de_la_tete(depart.x, depart.y), &self.corps) * self.r;
        }
        pos2(p.x / self.taille.x, p.y / self.taille.y)
    }

    /// Le maillage d'une couche sur le treillis : les cellules qu'elle couvre (pour un calque, les nœuds de sa
    /// géométrie ; pour la photo, tous), chaque nœud prenant dans sa texture le point de sa pose qui vient là — le
    /// nœud, déplacé par son flot à elle, et ce dont le corps glisse sous lui —, et la frange du bord qu'elle touche.
    /// `None` : elle ne couvre rien.
    fn maillage(&self, c: &Couche) -> Option<egui::Mesh> {
        let (ia, ja, ib, jb) = match &c.geometrie {
            Some(g) => (
                g.i0.max(self.i0),
                g.j0.max(self.j0),
                (g.i0 + g.ni).min(self.i0 + self.ni),
                (g.j0 + g.nj).min(self.j0 + self.nj),
            ),
            None => (self.i0, self.j0, self.i0 + self.ni, self.j0 + self.nj),
        };
        if ib < ia + 2 || jb < ja + 2 {
            return None;
        }
        let teinte = c.teinte();
        let uv = |n: usize, i: usize, j: usize| {
            let mut q = self.place[n] + self.glisse[n];
            if self.libre[n]
                && let Some(d) = c.geometrie.as_ref().and_then(|g| g.en(i, j))
            {
                q += vec2(d.dx, d.dy);
            }
            c.dans(pos2(q.x / self.taille.x, q.y / self.taille.y))
        };
        let largeur = (ib - ia) as u32;
        let mut m = egui::Mesh::with_texture(c.texture);
        for j in ja..jb {
            for i in ia..ib {
                let n = (j - self.j0) * self.ni + (i - self.i0);
                m.vertices.push(egui::epaint::Vertex { pos: self.ecran[n], uv: uv(n, i, j), color: teinte });
            }
        }
        for dj in 0..(jb - ja - 1) as u32 {
            for di in 0..largeur - 1 {
                let a = dj * largeur + di;
                m.indices.extend([a, a + 1, a + largeur, a + 1, a + largeur + 1, a + largeur]);
            }
        }
        // Le bord : entre deux nœuds du bord qui sont dans la couche, l'arc du cercle — la part du disque que leur corde
        // laisse, en éventail depuis le premier —, et sa frange, d'un point vers le dehors. Rien n'y bouge.
        let noeud = |n: usize| (self.i0 + n % self.ni, self.j0 + n / self.ni);
        let dedans = |n: usize| {
            let (i, j) = noeud(n);
            (ia..ib).contains(&i) && (ja..jb).contains(&j)
        };
        for paire in self.bord.windows(2) {
            let (a, b) = (paire[0], paire[1]);
            if a == b || !dedans(a) || !dedans(b) {
                continue;
            }
            let (da, db) = (self.ecran[a] - self.centre, self.ecran[b] - self.centre);
            let ecart = (db.angle() - da.angle() + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let pas = ((ecart.abs() / PAS_DE_L_ARC).ceil() as u32).max(1);
            let k = m.vertices.len() as u32;
            for s in 0..=pas {
                let (pos, prise) = match s {
                    0 => (self.ecran[a], uv(a, noeud(a).0, noeud(a).1)),
                    s if s == pas => (self.ecran[b], uv(b, noeud(b).0, noeud(b).1)),
                    s => {
                        let q = self.centre + Vec2::angled(da.angle() + ecart * s as f32 / pas as f32) * self.rayon;
                        (q, c.dans(self.image_de(q)))
                    }
                };
                let dehors = pos + (pos - self.centre).normalized();
                m.vertices.push(egui::epaint::Vertex { pos, uv: prise, color: teinte });
                m.vertices.push(egui::epaint::Vertex { pos: dehors, uv: prise, color: Color32::TRANSPARENT });
            }
            for s in 0..pas {
                let (p, q) = (k + 2 * s, k + 2 * s + 2);
                m.indices.extend([p, p + 1, q, p + 1, q + 1, q]);
                if s > 0 {
                    m.indices.extend([k, p, q]);
                }
            }
        }
        Some(m)
    }
}

#[cfg(test)]
mod essais {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    /// L'angle d'un point du cercle, depuis le bas (0 : en bas ; π : en haut).
    fn angle_sur_le_cercle(q: Pos2, centre: Pos2) -> f32 {
        let d = q - centre;
        d.x.atan2(d.y).abs()
    }

    #[test]
    fn la_tete_entraine_le_visage_d_un_bloc_et_jamais_le_bord() {
        // Au-dessus du menton, d'un bloc, sur toute la largeur (les cheveux aussi).
        for (u, v) in [(0.0, -0.6), (0.0, MENTON), (-0.65, -0.3), (0.95, -0.5)] {
            assert_eq!(poids_de_la_tete(u, v), 1.0, "({u}, {v})");
        }
        // Le long du cou, de moins en moins, jusqu'à sa base ; rien au col, ni aux épaules, ni sur le bord du disque.
        let cou: Vec<f32> = [-0.08, 0.0, 0.08, 0.15].iter().map(|&v| poids_de_la_tete(0.0, v)).collect();
        assert!(cou.windows(2).all(|p| p[0] > p[1]) && cou[0] < 1.0 && cou[3] > 0.0, "{cou:?}");
        assert_eq!(poids_de_la_tete(0.0, 0.2), 0.0);
        assert_eq!(poids_de_la_tete(-0.92, 0.08), 0.0, "l'épaule");
        for k in 0..=20 {
            let a = k as f32 / 20.0 * FRAC_PI_2;
            assert_eq!(poids_de_la_tete(a.cos(), a.sin()), 0.0, "le bord, à {a} rad sous le milieu");
        }
    }

    #[test]
    fn le_treillis_garde_la_forme_du_disque() {
        // Une image de 400 sur 300 ; le disque : un carré de 200 pixels, centré en (200, 160).
        let cadre = Rect::from_center_size(pos2(200.0 / 400.0, 160.0 / 300.0), vec2(200.0 / 400.0, 200.0 / 300.0));
        let (centre, rayon) = (pos2(500.0, 400.0), 50.0);
        for detoure in [false, true] {
            let t = Treillis::nouveau([400, 300], cadre, centre, rayon, detoure, &Pose::default());
            // Sans pose ni tête, chaque nœud se peint là où il est dans l'image, dans le disque (détouré, au-dessus de
            // son milieu, dans sa largeur seulement).
            for (n, &e) in t.ecran.iter().enumerate() {
                let q = t.place[n];
                let attendu = centre + (q - pos2(200.0, 160.0)) / 100.0 * rayon;
                assert!((e - attendu).length() < 1e-3, "{e:?} {attendu:?}");
                let d = (e - centre) / rayon;
                if !detoure || d.y > 0.0 {
                    assert!(d.length() <= 1.0 + 1e-4, "{d:?}");
                } else {
                    assert!(d.x.abs() <= 1.0 + 1e-4, "{d:?}");
                }
            }
            // Détouré, le treillis part du haut de l'image ; sinon, du haut du disque.
            let haut = t.ecran.iter().map(|e| e.y).fold(f32::INFINITY, f32::min);
            let attendu = if detoure { centre.y - 160.0 / 100.0 * rayon } else { centre.y - rayon };
            assert!((haut - attendu).abs() < 1e-3, "{haut} {attendu}");
            // Le bord : sur le cercle, chacun ; tout autour pour une photo, sous le milieu pour un portrait détouré.
            assert!(t.bord.iter().all(|&n| ((t.ecran[n] - centre).length() - rayon).abs() < 1e-3));
            let angles: Vec<f32> = t.bord.iter().map(|&n| angle_sur_le_cercle(t.ecran[n], centre)).collect();
            let plus_haut = angles.iter().copied().fold(0.0, f32::max);
            assert!(if detoure { plus_haut <= FRAC_PI_2 + 0.01 } else { plus_haut > 3.1 }, "{plus_haut}");
            // Peinte, la photo garde un bord rond : ce qui se voit d'elle reste dans le disque, sa frange s'efface un
            // point au-delà, et l'arc avance de deux degrés au plus, même en bas, où les rangées s'écartent sur lui.
            let photo = Couche {
                texture: egui::TextureId::default(),
                uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                part: 1.0,
                geometrie: None,
            };
            let m = t.maillage(&photo).unwrap();
            let mut arc = Vec::new();
            for v in &m.vertices {
                let d = (v.pos - centre).length();
                if v.color == Color32::TRANSPARENT {
                    assert!((d - rayon - 1.0).abs() < 1e-3, "{d}");
                    arc.push(angle_sur_le_cercle(v.pos, centre));
                } else if !detoure || v.pos.y > centre.y + 1e-3 {
                    assert!(d <= rayon + 1e-3, "{d}");
                }
            }
            arc.sort_by(f32::total_cmp);
            let saut = arc.windows(2).map(|p| p[1] - p[0]).fold(0.0, f32::max);
            assert!(saut <= PAS_DE_L_ARC + 1e-3, "{saut}");
        }
    }

    #[test]
    fn le_corps_bouge_dedans_et_glisse_sous_le_bord() {
        // Une image de 400 sur 300 ; le disque : un carré de 200 pixels, centré en (200, 160) ; un portrait détouré, au
        // calme, puis qui prend une inspiration et hausse les épaules, bien plus que jamais en vrai.
        let cadre = Rect::from_center_size(pos2(200.0 / 400.0, 160.0 / 300.0), vec2(200.0 / 400.0, 200.0 / 300.0));
        let (centre, rayon) = (pos2(500.0, 400.0), 50.0);
        let calme = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose::default());
        let corps = Corps { souffle: 3.0, epaules: 0.01, ..Corps::default() };
        let t = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose { corps, ..Pose::default() });
        let noeud = |x: usize, y: usize| (y / PAS - t.j0) * t.ni + (x / PAS - t.i0);
        let bouge = |n: usize| (t.ecran[n] - calme.ecran[n]) / rayon;
        // Loin du bord, chaque nœud bouge d'autant que le corps l'emporte : la poitrine et les épaules montent, et
        // s'écartent un peu, les épaules plus encore ; la tête monte, portée par elles, à moitié de ce que le souffle
        // soulève ; le milieu du cou, avec le souffle seul.
        let epaule = noeud(248, 192);
        let attendu = deplacement_du_corps((t.place[epaule] - t.c) / t.r, 0.0, &corps);
        assert!(attendu.y < -0.018 - 0.005 && attendu.x > 0.0, "{attendu:?}");
        assert!((bouge(epaule) - attendu).length() < 1e-5 && t.glisse[epaule] == Vec2::ZERO, "{:?}", bouge(epaule));
        let tete = bouge(noeud(200, 96));
        assert!((tete.y + SOUFFLE_DE_LA_TETE * SOUFFLE * 3.0).abs() < 1e-5 && tete.x.abs() < 1e-6, "{tete:?}");
        let cou = bouge(noeud(200, 168));
        let buste = 1.0 - poids_de_la_tete(0.0, 0.08);
        let souffle = SOUFFLE * 3.0 * (SOUFFLE_DE_LA_TETE + (1.0 - SOUFFLE_DE_LA_TETE) * buste);
        assert!((cou.y + souffle).abs() < 1e-5, "{cou:?} {souffle}");
        // Le bord ne bouge pas : le corps glisse dessous, et ce qui s'y voit vient d'autant plus bas.
        for &n in &t.bord {
            assert!(bouge(n).length() < 1e-6, "{:?}", t.place[n]);
            let ici = t.image_de(t.ecran[n]);
            let vu = t.place[n] + t.glisse[n];
            assert!((pos2(ici.x * 400.0, ici.y * 300.0) - vu).length() < 1e-3, "{ici:?} {vu:?}");
        }
        assert!(t.bord.iter().any(|&n| t.glisse[n].y > 1.0), "le corps monte : on voit plus bas");
        // Ce qui se peint sous le milieu du disque reste dedans : rien ne dépasse le bord.
        let photo = Couche {
            texture: egui::TextureId::default(),
            uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            part: 1.0,
            geometrie: None,
        };
        for v in t.maillage(&photo).unwrap().vertices {
            if v.color != Color32::TRANSPARENT && v.pos.y > centre.y {
                assert!((v.pos - centre).length() <= rayon + 1e-3, "{:?}", v.pos);
            }
        }
        // Une photo ne bouge pas par son corps (son souffle la grandit dans le disque).
        let photo = Treillis::nouveau([400, 300], cadre, centre, rayon, false, &Pose { corps, ..Pose::default() });
        let sans = Treillis::nouveau([400, 300], cadre, centre, rayon, false, &Pose::default());
        assert!(photo.ecran.iter().zip(&sans.ecran).all(|(a, b)| (*a - *b).length() < 1e-6));
    }

    #[test]
    fn le_torse_se_penche_et_le_bord_ne_bouge_pas() {
        // Le même disque ; le torse penché du côté de la tête (vers la droite de l'écran), redressé, et vers la
        // personne, bien plus que jamais en vrai.
        let cadre = Rect::from_center_size(pos2(200.0 / 400.0, 160.0 / 300.0), vec2(200.0 / 400.0, 200.0 / 300.0));
        let (centre, rayon) = (pos2(500.0, 400.0), 50.0);
        let calme = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose::default());
        let corps = Corps { roulis: 0.03, avance: 0.02, monte: 0.01, ..Corps::default() };
        let t = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose { corps, ..Pose::default() });
        let noeud = |x: usize, y: usize| (y / PAS - t.j0) * t.ni + (x / PAS - t.i0);
        let bouge = |n: usize| (t.ecran[n] - calme.ecran[n]) / rayon;
        // Penché du côté de la tête : le haut du corps va vers la droite, l'épaule de gauche monte, celle de droite
        // descend ; plus près, il grandit : l'épaule s'écarte.
        let (gauche, droite) = (bouge(noeud(136, 176)), bouge(noeud(264, 176)));
        assert!(gauche.y < droite.y - 0.02 && gauche.x > -0.01 && droite.x > 0.02, "{gauche:?} {droite:?}");
        // Plus près, la tête grandit autour des yeux : son haut monte, son menton descend ; le roulis et le
        // redressement du torse ne la touchent pas.
        let (haut, menton) = (bouge(noeud(200, 64)), bouge(noeud(200, 144)));
        assert!((haut.y - (-0.96 + 0.55) * 0.02).abs() < 1e-4 && (menton.y - (-0.16 + 0.55) * 0.02).abs() < 1e-4);
        assert!(haut.x.abs() < 1e-5, "{haut:?}");
        // Le bord ne bouge pas, et ce qui se peint sous le milieu du disque reste dedans.
        assert!(t.bord.iter().all(|&n| bouge(n).length() < 1e-6));
        let photo = Couche {
            texture: egui::TextureId::default(),
            uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            part: 1.0,
            geometrie: None,
        };
        for v in t.maillage(&photo).unwrap().vertices {
            if v.color != Color32::TRANSPARENT && v.pos.y > centre.y {
                assert!((v.pos - centre).length() <= rayon + 1e-3, "{:?}", v.pos);
            }
        }
    }

    #[test]
    fn les_cheveux_en_retard_laissent_le_visage_et_le_bord() {
        // Une image de 400 sur 300, opaque ; le disque : un carré de 200 pixels, centré en (200, 160) ; des cheveux à
        // gauche (jusqu'à 180), le visage à droite. Les longueurs en retard vers la gauche : à mi-longueur d'un
        // centième du rayon, aux pointes de deux.
        let cadre = Rect::from_center_size(pos2(200.0 / 400.0, 160.0 / 300.0), vec2(200.0 / 400.0, 200.0 / 300.0));
        let (centre, rayon) = (pos2(500.0, 400.0), 50.0);
        let photo = egui::ColorImage::new([400, 300], vec![Color32::from_rgb(120, 80, 50); 400 * 300]);
        let masque = egui::ColorImage::new(
            [400, 300],
            (0..400 * 300).map(|k| if k % 400 < 180 { Color32::WHITE } else { Color32::BLACK }).collect(),
        );
        let souplesse = std::sync::Arc::new(super::super::cheveux::Souplesse::de(&photo, &masque));
        let cheveux = super::super::cheveux::Retard::d_essai(souplesse, -0.01, -0.02);
        let calme = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose::default());
        let t = Treillis::nouveau([400, 300], cadre, centre, rayon, true, &Pose { cheveux, ..Pose::default() });
        let noeud = |x: usize, y: usize| (y / PAS - t.j0) * t.ni + (x / PAS - t.i0);
        let bouge = |n: usize| (t.ecran[n] - calme.ecran[n]) / rayon;
        // Les longueurs, au-dessus du menton : vers la gauche, de côté seulement, entre le retard du milieu et celui
        // des pointes.
        let longueur = bouge(noeud(136, 128));
        assert!(longueur.x < -0.01 && longueur.x > -0.02 && longueur.y == 0.0, "{longueur:?}");
        // Le visage, les cheveux au-dessus des racines, le bord du disque : rien.
        assert_eq!(bouge(noeud(232, 128)), Vec2::ZERO, "le visage");
        assert_eq!(bouge(noeud(136, 72)), Vec2::ZERO, "au-dessus des racines");
        assert!(t.bord.iter().all(|&n| bouge(n) == Vec2::ZERO), "le bord");
    }

    #[test]
    fn la_profondeur_est_celle_d_une_tete() {
        // Le bout du nez, le plus en avant ; puis le visage (entre les yeux, la bouche, le menton), les pommettes, les
        // cheveux qui l'encadrent ; le haut du crâne et le cou, moins ; rien au bord des cheveux ni au-dessus du crâne.
        let z = |u: f32, v: f32| Rangee::de(v).profondeur(u);
        let (nez, yeux, bouche, menton) = (z(-0.025, -0.39), z(-0.015, -0.58), z(-0.015, -0.27), z(-0.015, -0.08));
        assert!(nez > yeux + 0.04 && nez > bouche && nez > menton, "{nez} {yeux} {bouche} {menton}");
        assert!([yeux, bouche, menton].iter().all(|p| (0.38..0.5).contains(p)), "{yeux} {bouche} {menton}");
        let (pommette, tempe, bord) = (z(0.2, -0.45), z(0.38, -0.6), z(0.62, -0.6));
        assert!(yeux > pommette && pommette > tempe && tempe > 0.2 && bord < 0.12, "{pommette} {tempe} {bord}");
        // Le cou, à 5 cm en avant de l'axe, le menton à 9.
        let (crane, cou) = (z(-0.015, -1.15), z(-0.015, 0.1));
        assert!(crane < 0.6 * yeux && cou < 0.65 * menton, "{crane} {cou}");
        assert!(z(-0.015, -1.3) == 0.0 && z(0.9, -0.6) == 0.0);
        // Douce partout : d'un point au suivant (un deux-centième du rayon), jamais plus d'un soixante-dixième — une
        // pente de trois au plus : à cinq degrés, l'image se resserre ou s'étire d'un quart au plus, sans pli.
        for j in 0..=320 {
            let v = -1.3 + j as f32 * 0.005;
            for i in 0..=360 {
                let u = -0.9 + i as f32 * 0.005;
                let (a, b, c) = (z(u, v), z(u + 0.005, v), z(u, v + 0.005));
                assert!((a - b).abs() < 0.015 && (a - c).abs() < 0.015, "({u}, {v}) : {a} {b} {c}");
            }
        }
    }

    #[test]
    fn la_tete_tourne_et_hoche_comme_une_tete() {
        // Une image de 800 sur 700 ; le disque : un carré de 600 pixels, centré en (400, 430) ; un portrait détouré,
        // au calme, puis la tête tournée de quatre degrés (le visage vers la droite de l'écran), puis baissée d'autant.
        let cadre = Rect::from_center_size(pos2(400.0 / 800.0, 430.0 / 700.0), vec2(600.0 / 800.0, 600.0 / 700.0));
        let (centre, rayon) = (pos2(1000.0, 1000.0), 300.0);
        let pose = |tete: Tete| Pose { tete, ..Pose::default() };
        let calme = Treillis::nouveau([800, 700], cadre, centre, rayon, true, &Pose::default());
        let angle = 4f32.to_radians();
        let tournee =
            Treillis::nouveau([800, 700], cadre, centre, rayon, true, &pose(Tete { lacet: angle, ..Tete::default() }));
        let baissee = Treillis::nouveau(
            [800, 700],
            cadre,
            centre,
            rayon,
            true,
            &pose(Tete { tangage: angle, ..Tete::default() }),
        );
        // Le nœud le plus proche d'un point du portrait (en parts du rayon), et son chemin à l'écran (en parts du
        // rayon).
        let noeud = |u: f32, v: f32| {
            let p = calme.c + vec2(u, v) * calme.r;
            let (i, j) = ((p.x / PAS as f32).round() as usize, (p.y / PAS as f32).round() as usize);
            (j - calme.j0) * calme.ni + (i - calme.i0)
        };
        let chemin = |t: &Treillis, n: usize| (t.ecran[n] - calme.ecran[n]) / rayon;
        let depart = |n: usize| (calme.place[n] - calme.c) / calme.r;
        // Tournée : chaque point de côté, de sa profondeur ; le nez plus que les pommettes, plus que les cheveux qui
        // encadrent le visage ; le bord des cheveux, à peine ; rien en hauteur.
        let nez = noeud(-0.025, -0.39);
        let attendu = Rangee::de(depart(nez).y).profondeur(depart(nez).x) * angle.sin();
        assert!((chemin(&tournee, nez).x - attendu).abs() < 0.05 * attendu && chemin(&tournee, nez).y.abs() < 1e-4);
        let x = |u: f32, v: f32| chemin(&tournee, noeud(u, v)).x;
        assert!(x(-0.025, -0.39) > x(0.2, -0.45) && x(0.2, -0.45) > x(0.4, -0.6) && x(0.62, -0.6) < 0.4 * attendu);
        // Le côté qui s'éloigne (la droite de l'écran) se resserre, l'autre s'élargit.
        let large = |t: &Treillis, a: usize, b: usize| t.ecran[b].x - t.ecran[a].x;
        let (droite, gauche) = ((noeud(0.15, -0.45), noeud(0.5, -0.45)), (noeud(-0.5, -0.45), noeud(-0.15, -0.45)));
        assert!(large(&tournee, droite.0, droite.1) < large(&calme, droite.0, droite.1) - 0.005 * rayon);
        assert!(large(&tournee, gauche.0, gauche.1) > large(&calme, gauche.0, gauche.1) + 0.005 * rayon);
        // Baissée : chaque point vers le bas, de sa profondeur ; le visage plus que le haut du crâne ; le menton se
        // rapproche du cou ; rien de côté.
        let y = |u: f32, v: f32| chemin(&baissee, noeud(u, v)).y;
        assert!((y(-0.025, -0.39) - attendu).abs() < 0.05 * attendu && chemin(&baissee, nez).x.abs() < 1e-4);
        assert!(y(-0.015, -0.6) > 1.8 * y(-0.015, -1.15) && y(-0.015, -0.6) > 2.0 * y(0.6, -0.6));
        let haut = |t: &Treillis, a: usize, b: usize| t.ecran[b].y - t.ecran[a].y;
        let (menton, cou) = (noeud(-0.015, -0.06), noeud(-0.015, 0.06));
        assert!(haut(&baissee, menton, cou) < haut(&calme, menton, cou) - 0.004 * rayon);
        // Ni l'une ni l'autre ne bouge le bord du disque ; aucune ne plie l'image, même au double de l'angle : d'un
        // nœud libre au suivant, l'écran avance toujours, de gauche à droite et de haut en bas.
        for t in [&tournee, &baissee] {
            assert!(t.bord.iter().all(|&n| chemin(t, n).length() < 1e-6));
        }
        for tete in [Tete { lacet: 2.0 * angle, ..Tete::default() }, Tete { tangage: -2.0 * angle, ..Tete::default() }]
        {
            let t = Treillis::nouveau([800, 700], cadre, centre, rayon, true, &pose(tete));
            for j in 0..t.nj {
                for i in 0..t.ni {
                    let n = j * t.ni + i;
                    if i + 1 < t.ni && t.libre[n] && t.libre[n + 1] {
                        assert!(t.ecran[n + 1].x > t.ecran[n].x + 0.5 * PAS as f32, "{:?}", depart(n));
                    }
                    if j + 1 < t.nj && t.libre[n] && t.libre[n + t.ni] {
                        assert!(t.ecran[n + t.ni].y > t.ecran[n].y + 0.5 * PAS as f32, "{:?}", depart(n));
                    }
                }
            }
        }
    }

    #[test]
    fn un_calque_pose_se_peint_tel_quel_et_le_melange_tombe_juste() {
        // Une image de 160 sur 160, le disque entier ; un calque qui couvre les nœuds (4…12, 4…12), où sa pose a bougé
        // de 6 pixels vers le bas, son masque plein au milieu.
        let cadre = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        let (i0, j0, ni, nj) = (4, 4, 9, 9);
        let noeuds = (0..ni * nj)
            .map(|k| {
                let (di, dj) = (k % ni, k / ni);
                let plein = (2..7).contains(&di) && (2..7).contains(&dj);
                Noeud { dx: 0.0, dy: 6.0, masque: if plein { 1.0 } else { 0.0 } }
            })
            .collect();
        let g = std::sync::Arc::new(Geometrie { i0, j0, ni, nj, noeuds });
        let couche = |part: f32| Couche {
            texture: egui::TextureId::default(),
            uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            part,
            geometrie: Some(g.clone()),
        };
        let (centre, rayon) = (pos2(80.0, 80.0), 80.0);
        let pose = |calques: Vec<Couche>| {
            let mut p = Pose::default();
            p.couches[1] = calques;
            p
        };
        // Entier, le calque se peint tel qu'il est : chaque nœud plein, à l'endroit d'où il prend sa pose.
        let t = Treillis::nouveau([160, 160], cadre, centre, rayon, false, &pose(vec![couche(1.0)]));
        let m = t.maillage(&couche(1.0)).unwrap();
        let milieu = (8 - j0) * ni + (8 - i0);
        let v = m.vertices[milieu];
        assert!((v.pos - pos2(v.uv.x * 160.0, v.uv.y * 160.0)).length() < 1e-3, "{v:?}");
        assert!((v.pos.y - (64.0 + 6.0)).abs() < 1e-3, "{v:?}");
        // À moitié : le treillis à mi-chemin, et le calque y prend le point de sa pose qui vient là.
        let t = Treillis::nouveau([160, 160], cadre, centre, rayon, false, &pose(vec![couche(0.5)]));
        let n = (8 - t.j0) * t.ni + (8 - t.i0);
        assert!((t.ecran[n].y - 67.0).abs() < 1e-3 && t.flot[n] == vec2(0.0, 3.0), "{:?}", t.ecran[n]);
        let m = t.maillage(&couche(0.5)).unwrap();
        assert!((m.vertices[milieu].uv.y * 160.0 - 70.0).abs() < 1e-3);
        // Hors de son masque, le calque ne déplace rien.
        let bord = (4 - t.j0) * t.ni + (4 - t.i0);
        assert_eq!(t.flot[bord], Vec2::ZERO);
        // Un calque hors du treillis ne se peint pas.
        let loin = Couche {
            geometrie: Some(std::sync::Arc::new(Geometrie {
                i0: 400,
                j0: 400,
                ni: 3,
                nj: 3,
                noeuds: vec![Noeud::default(); 9],
            })),
            ..couche(1.0)
        };
        assert!(t.maillage(&loin).is_none());
    }
}
