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
//! descend. Puis la tête : penchée autour de la base du cou, elle entraîne tout ce qui est au-dessus du menton d'un seul
//! bloc — le visage ne se tord jamais —, le cou et les cheveux de moins en moins, et rien des épaules ni du bord du
//! disque ([`poids_de_la_tete`]). Puis le corps ([`deplacement_du_corps`]) : son souffle soulève la poitrine et les
//! épaules, la tête portée par elles. Le bord du disque ne bouge jamais, et les épaules le touchent : loin de lui, le
//! corps déplace le treillis ; près de lui, il glisse dessous, comme derrière le cadre d'un médaillon — chaque couche
//! va chercher dans son image le point qui vient là ([`BORD`]). Un seul maillage par couche, du haut de l'image au bas
//! du disque : aucune couture.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};

use super::flot::{self, Gris};
use super::{Corps, Couche, Pose, Tete, Visage, de_pres, nettete};
use crate::natif::theme::erp;

/// L'écart des nœuds du treillis, en pixels de l'image : le flot est lissé sur six pixels, et la tête se plie sur des
/// dizaines ; huit suffisent, et le disque garde un bord rond.
pub(super) const PAS: usize = 8;
/// Le pivot de la tête, en parts du rayon depuis le centre du disque (y vers le bas) : la base du cou, où une tête se
/// penche.
const PIVOT: Vec2 = vec2(0.0, 0.15);
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
        let (s, co) = pose.tete.roulis.sin_cos();
        let Tete { x: tx, y: ty, .. } = pose.tete;
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
                        let e = d - PIVOT;
                        let penche = vec2(e.x * co - e.y * s, e.x * s + e.y * co);
                        d += (penche - e + vec2(tx, ty)) * poids;
                    }
                    let corps = deplacement_du_corps(depart, poids, &pose.corps);
                    let loin = lisse(distance_au_bord(depart) / BORD);
                    d += corps * loin;
                    glisse = corps * ((loin - 1.0) * r);
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
