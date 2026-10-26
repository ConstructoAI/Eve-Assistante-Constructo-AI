//! Les cheveux d'Eve, qui suivent sa tête avec un temps de retard (« le réalisme facial, physique ») : ses longues
//! mèches ondulées pendent de ses tempes jusque sur ses épaules ; quand sa tête penche ou se déplace, leurs racines la
//! suivent tout de suite, leurs longueurs traînent un peu derrière, se balancent légèrement et se posent. Comme un
//! pendule que la tête emporte : le milieu des longueurs suit les racines, les pointes suivent le milieu
//! ([`Balancier`]), chacun sur un ressort un peu moins amorti que ceux du visage. Seul ce qui va de côté compte : une
//! mèche qui pend est tirée par sa racine quand la tête monte ou descend — elle la suit aussitôt —, mais elle se
//! balance quand sa racine va de côté. Et rien ne flotte : la tête immobile, les cheveux le sont aussi.
//!
//! Où sont les cheveux : un masque fait d'avance d'après la photo, comme les poses (`portrait-cheveux.png`, à côté du
//! portrait ; `outils\cheveux_du_portrait.py`) ; d'où la souplesse de chaque nœud du treillis du rendu
//! ([`Souplesse`]) : nulle sur ce qui ne pend pas (le visage, le cou, les vêtements) et tout contre, pleine dans les
//! cheveux qui s'en écartent. Le rendu déplace chaque nœud de ce retard ([`Retard::ecart`]), à part de la tête :
//! autant qu'elle emporte le nœud, plus aux pointes qu'à mi-longueur, rien au-dessus des racines.

use std::sync::Arc;

use eframe::egui::{self, Vec2, vec2};

use super::flou;
use super::mimique::{Moment, lisse};
use super::rendu::{Mouvement, PAS, Rangee};
use super::{Pose, Tete, Visage};

/// La hauteur des racines des longueurs — les tempes : au-dessus, les cheveux tiennent au crâne et suivent la tête d'un
/// bloc — et la longueur jusqu'aux pointes qui pendent librement — le menton —, en parts du rayon (depuis le centre du
/// disque, vers le bas).
const RACINES: f32 = -0.65;
const LONGUEUR: f32 = 0.55;
/// Les balanciers : le milieu des longueurs suit les racines, les pointes suivent le milieu. Leurs pulsations (par
/// seconde : une mèche d'une quinzaine de centimètres qui pend, puis d'une trentaine) et leurs amortissements (moins
/// que ceux du visage : elles dépassent un peu, puis se posent, freinées par l'air et par les autres mèches).
const W_MILIEU: f32 = 9.0;
const Z_MILIEU: f32 = 0.5;
const W_POINTES: f32 = 7.0;
const Z_POINTES: f32 = 0.4;
/// Ce qui ne pend pas (opaque, et pas des cheveux) retient les cheveux qui le touchent : ils ne sont tout à fait
/// libres qu'à une fois et quart cette distance de lui (le grand côté de l'image divisé par ce nombre : 24 pixels sur
/// 768, trois nœuds du treillis) — une mèche posée sur la joue suit le visage. Ses filets de moins de deux fois
/// [`ROGNE`] pixels de large ne retiennent rien : la frange des cheveux sur le fond, que le masque, plus flou que le
/// détourage, laisse hors des cheveux.
const ECART: usize = 32;
const ROGNE: usize = 3;

/// Un balancier : un ressort un peu moins amorti que ceux du visage ([`super::mimique::Ressort`]) — il dépasse un peu
/// sa cible et y revient, comme une mèche qui se balance et se pose ; sa valeur et sa vitesse.
#[derive(Clone, Copy, Default)]
struct Balancier {
    x: f32,
    v: f32,
}

impl Balancier {
    /// Avance de `dt` secondes vers `cible`, à la pulsation `w` (par seconde) et à l'amortissement `z` (moins de 1),
    /// par la solution exacte de l'oscillateur — juste quel que soit le pas ; rend sa valeur.
    fn vers(&mut self, cible: f32, w: f32, z: f32, dt: f32) -> f32 {
        let d = self.x - cible;
        let wd = w * (1.0 - z * z).sqrt();
        let e = (-z * w * dt).exp();
        let (s, c) = (wd * dt).sin_cos();
        let b = (self.v + z * w * d) / wd;
        let p = d * c + b * s;
        self.x = cible + e * p;
        self.v = e * (wd * (b * c - d * s) - z * w * p);
        self.x
    }
}

/// Les longueurs, d'une image à l'autre : leur milieu et leurs pointes (où ils sont de côté, en part du rayon, et leur
/// vitesse) ; parties ou non — à la première image, et quand les animations reviennent, elles partent de là où sont les
/// racines : rien ne saute.
#[derive(Default)]
pub(super) struct Cheveux {
    milieu: Balancier,
    pointes: Balancier,
    parties: bool,
}

impl Cheveux {
    /// Les longueurs à cette image, sous la tête de la pose (que ses gestes ont posée) : elles suivent ses racines, et
    /// leur retard va à la pose, avec la souplesse du visage `v`. Sans masque des cheveux, ou sans les animations de
    /// Windows, aucun retard : les cheveux suivent la tête d'un bloc.
    pub(super) fn avancer(&mut self, m: &Moment, v: &Visage, pose: &mut Pose) {
        let Some(souplesse) = v.cheveux.as_ref().filter(|_| m.vivant) else {
            self.parties = false;
            return;
        };
        let (milieu, pointes) = self.suivre(racines(pose.tete), m.dt);
        pose.cheveux = Retard { souplesse: Some(souplesse.clone()), milieu, pointes };
    }

    /// Les longueurs suivent leurs `racines` (où la tête les emporte, de côté) pendant `dt` secondes : leur retard sur
    /// elles, à mi-longueur et aux pointes.
    fn suivre(&mut self, racines: f32, dt: f32) -> (f32, f32) {
        if !self.parties {
            let au_repos = Balancier { x: racines, v: 0.0 };
            (self.milieu, self.pointes, self.parties) = (au_repos, au_repos, true);
        }
        let milieu = self.milieu.vers(racines, W_MILIEU, Z_MILIEU, dt);
        let pointes = self.pointes.vers(milieu, W_POINTES, Z_POINTES, dt);
        (milieu - racines, pointes - racines)
    }
}

/// De part et d'autre du milieu du visage, où sont les racines des longueurs (en part du rayon) : aux tempes.
const TEMPES: f32 = 0.45;

/// Où la tête `t` emporte les racines des longueurs, de côté (en part du rayon) : ce que son mouvement fait des deux
/// tempes ([`Mouvement`] : tournée et hochée en trois dimensions, chacune de sa profondeur — [`Rangee`] —, puis
/// penchée autour de la base du cou, [`super::rendu::PIVOT`], et déplacée), en moyenne. Penchée, elle les déplace
/// d'autant qu'elles sont au-dessus du pivot ; tournée — un « non » —, de leur profondeur : les longueurs traînent
/// aussi derrière lui.
fn racines(t: Tete) -> f32 {
    let (m, rangee) = (Mouvement::de(&t), Rangee::de(RACINES));
    let de_cote = |u: f32| m.deplacement(vec2(u, RACINES), rangee.profondeur(u)).x;
    0.5 * (de_cote(-TEMPES) + de_cote(TEMPES))
}

/// Le retard des longueurs sur leurs racines, à une image (de côté, en part du rayon) : à mi-longueur, aux pointes ; et
/// la souplesse de chaque nœud du treillis. Aucun, d'office : les cheveux suivent la tête d'un bloc.
#[derive(Clone, Default)]
pub(crate) struct Retard {
    souplesse: Option<Arc<Souplesse>>,
    milieu: f32,
    pointes: f32,
}

impl Retard {
    /// Un retard donné, pour l'essai du portrait : celui du `milieu` et des `pointes`, sur la `souplesse` du visage.
    pub(super) fn d_essai(souplesse: Arc<Souplesse>, milieu: f32, pointes: f32) -> Retard {
        Retard { souplesse: Some(souplesse), milieu, pointes }
    }

    /// Ce dont le retard déplace le nœud (i, j) de l'image, à la hauteur `v` (en parts du rayon depuis le centre du
    /// disque, vers le bas), en part du rayon : rien au-dessus des racines, le retard du milieu à mi-longueur, celui
    /// des pointes au bas des longueurs, à la souplesse du nœud. Le rendu le règle sur ce que la tête emporte du nœud.
    pub(super) fn ecart(&self, i: usize, j: usize, v: f32) -> Vec2 {
        let Some(souplesse) = &self.souplesse else { return Vec2::ZERO };
        let libre = souplesse.en(i, j);
        if libre <= 0.0 {
            return Vec2::ZERO;
        }
        let l = lisse((v - RACINES) / LONGUEUR);
        let x =
            if l < 0.5 { 2.0 * l * self.milieu } else { self.milieu + (2.0 * l - 1.0) * (self.pointes - self.milieu) };
        vec2(x * libre, 0.0)
    }
}

/// La souplesse des cheveux à chaque nœud du treillis du rendu (un tous les [`PAS`] pixels de l'image, rangée par
/// rangée, `ni` par rangée, `nj` rangées) : de 0 (ce qui ne pend pas, et tout contre) à 1 (les cheveux qui s'en
/// écartent, et le vide autour, qui ne se voit pas).
pub(crate) struct Souplesse {
    ni: usize,
    nj: usize,
    noeuds: Vec<f32>,
}

impl Souplesse {
    /// D'après la `photo` (son détourage) et le `masque` des cheveux (de la même taille ; en gris, blanc : des
    /// cheveux) : ce qui est opaque sans être des cheveux ne pend pas — rogné d'abord de ses filets ([`ROGNE`]) —, puis
    /// élargi de la moitié de l'[`ECART`] et adouci sur le reste, sans jamais rien rendre de souple de ce qu'il était :
    /// les cheveux qui le touchent le suivent.
    pub(super) fn de(photo: &egui::ColorImage, masque: &egui::ColorImage) -> Souplesse {
        let [l, h] = photo.size;
        let mut fixe: Vec<f32> = photo
            .pixels
            .iter()
            .zip(&masque.pixels)
            .map(|(p, m)| if p.a() >= 128 && m.r() < 128 { 1.0 } else { 0.0 })
            .collect();
        flou(&mut fixe, l, h, ROGNE);
        fixe.iter_mut().for_each(|f| *f = if *f > 0.9 { 1.0 } else { 0.0 });
        // Élargi d'une demi-fenêtre (le flou en boîte, multiplié par quatre), puis adouci sur moins que ce qui a été
        // rogné et élargi : ce qui ne pendait pas reste entier.
        let r = (l.max(h) / ECART).max(2 * ROGNE + 2);
        flou(&mut fixe, l, h, r);
        fixe.iter_mut().for_each(|f| *f = (*f * 4.0).min(1.0));
        flou(&mut fixe, l, h, r / 2 - ROGNE);
        // Ce qui ne pend pas ne bouge pas du tout : les sommes des flous y laissent des miettes, ôtées.
        let (ni, nj, fixe) = (l.div_ceil(PAS), h.div_ceil(PAS), &fixe);
        let souple = move |i: usize, j: usize| {
            let s = 1.0 - fixe[j * PAS * l + i * PAS];
            if s < 1e-3 { 0.0 } else { s }
        };
        let noeuds = (0..nj).flat_map(|j| (0..ni).map(move |i| souple(i, j))).collect();
        Souplesse { ni, nj, noeuds }
    }

    /// La souplesse au nœud (i, j) de l'image ; hors d'elle, aucune.
    fn en(&self, i: usize, j: usize) -> f32 {
        if i < self.ni && j < self.nj { self.noeuds[j * self.ni + i] } else { 0.0 }
    }
}

#[cfg(test)]
mod essais {
    use eframe::egui::Color32;

    use super::super::rendu::PIVOT;
    use super::*;

    #[test]
    fn un_balancier_depasse_un_peu_et_se_pose() {
        // Sauté d'un coup vers 1, aux réglages des pointes : il dépasse d'un quart, puis se pose ; le même chemin à
        // grands ou à petits pas. (La tête ne saute jamais : en vrai, les pointes dépassent bien moins.)
        let (mut a, mut b) = (Balancier::default(), Balancier::default());
        let mut plus_haut = 0.0f32;
        for _ in 0..180 {
            plus_haut = plus_haut.max(a.vers(1.0, W_POINTES, Z_POINTES, 1.0 / 60.0));
        }
        assert!(plus_haut > 1.1 && plus_haut < 1.3, "{plus_haut}");
        assert!((a.x - 1.0).abs() < 0.01 && a.v.abs() < 0.05, "{} {}", a.x, a.v);
        for _ in 0..90 {
            b.vers(1.0, W_POINTES, Z_POINTES, 1.0 / 30.0);
        }
        assert!((a.x - b.x).abs() < 1e-4 && (a.v - b.v).abs() < 1e-3, "{} et {}", a.x, b.x);
    }

    /// Les longueurs à 60 images par seconde, leurs racines à chaque image : leur retard, à mi-longueur et aux pointes.
    fn filmer(c: &mut Cheveux, racines: impl Iterator<Item = f32>) -> Vec<(f32, f32)> {
        racines.map(|r| c.suivre(r, 1.0 / 60.0)).collect()
    }

    #[test]
    fn les_cheveux_ne_bougent_que_si_la_tete_bouge() {
        // La tête immobile, déjà penchée : aucun retard, dès la première image.
        let mut c = Cheveux::default();
        assert!(filmer(&mut c, std::iter::repeat_n(0.02, 120)).iter().all(|&(m, p)| m == 0.0 && p == 0.0));
        // Elle change de posture (ses racines vont de 0,02 à 0,05 sur le ressort de la tête, en trois quarts de
        // seconde) : les longueurs traînent derrière — les pointes plus que le milieu —, puis se posent.
        let mut ressort = super::super::mimique::Ressort { x: 0.02, v: 0.0 };
        let racines: Vec<f32> = (0..300).map(|_| ressort.vers(0.05, 5.0, 1.0 / 60.0)).collect();
        let retard = filmer(&mut c, racines.into_iter());
        let (milieu, pointes) = retard.iter().fold((0.0f32, 0.0f32), |(a, b), &(m, p)| (a.min(m), b.min(p)));
        assert!(pointes < 1.5 * milieu && pointes < -0.004, "en retard : {milieu} {pointes}");
        // Les pointes dépassent un peu quand la tête s'arrête, puis se posent ; et ne bougent plus.
        assert!(retard.iter().any(|&(_, p)| p > 0.0002), "elles dépassent un peu");
        let fin = retard[240..].iter().fold(0.0f32, |a, &(m, p)| a.max(m.abs()).max(p.abs()));
        assert!(fin < 0.0005, "{fin}");
        let calme = filmer(&mut c, std::iter::repeat_n(0.05, 600));
        assert!(calme.last().is_some_and(|&(m, p)| m.abs() < 1e-6 && p.abs() < 1e-6), "{:?}", calme.last());
    }

    #[test]
    fn la_tete_emporte_les_racines() {
        // Penchée, elle les emporte d'autant qu'elles sont au-dessus du pivot (au cosinus près, que la moyenne des deux
        // tempes efface) ; déplacée, d'autant ; tournée, de leur profondeur, du côté où elle tourne.
        let penchee = Tete { roulis: 0.05, ..Tete::default() };
        assert!((racines(penchee) - 0.05 * (PIVOT.y - RACINES)).abs() < 1e-3, "{}", racines(penchee));
        assert!((racines(Tete { x: 0.01, ..Tete::default() }) - 0.01).abs() < 1e-6);
        let tournee = racines(Tete { lacet: 0.04, ..Tete::default() });
        let profondeur = Rangee::de(RACINES).profondeur(TEMPES);
        assert!(
            tournee > 0.0 && (tournee - 0.04f32.sin() * profondeur).abs() < 0.2 * tournee,
            "{tournee} {profondeur}"
        );
    }

    /// Une photo de 480 sur 480 et son masque : un visage opaque au milieu (un carré de 120, de 180 à 300), des cheveux
    /// opaques autour (de 60 à 420) — leur frange sur le fond, d'un pixel, hors du masque —, le fond transparent.
    fn photo_et_masque() -> (egui::ColorImage, egui::ColorImage) {
        let n = 480;
        let (mut photo, mut masque) = (Vec::new(), Vec::new());
        for y in 0..n {
            for x in 0..n {
                let dedans = |a: usize, b: usize| (a..b).contains(&x) && (a..b).contains(&y);
                let (visage, cheveux, frange) = (dedans(180, 300), dedans(61, 419), dedans(60, 420));
                photo.push(if frange { Color32::from_rgb(120, 80, 50) } else { Color32::TRANSPARENT });
                masque.push(if cheveux && !visage { Color32::WHITE } else { Color32::BLACK });
            }
        }
        (egui::ColorImage::new([n, n], photo), egui::ColorImage::new([n, n], masque))
    }

    #[test]
    fn la_souplesse_epargne_le_visage() {
        let (photo, masque) = photo_et_masque();
        let s = Souplesse::de(&photo, &masque);
        assert_eq!((s.ni, s.nj), (60, 60));
        // Sur le visage, jusqu'à son bord : aucune ; dans les cheveux qui s'en écartent, sur leur frange et sur le
        // fond : entière.
        assert_eq!(s.en(30, 30), 0.0, "le milieu du visage");
        assert_eq!((s.en(180 / PAS + 1, 30), s.en(296 / PAS, 30)), (0.0, 0.0), "les bords du visage");
        assert!(s.en(80 / PAS, 30) > 0.99, "les cheveux loin du visage : {}", s.en(80 / PAS, 30));
        assert!(s.en(64 / PAS, 30) > 0.99, "près de leur frange : {}", s.en(64 / PAS, 30));
        assert_eq!(s.en(1, 1), 1.0, "le fond");
        // De la joue vers le dehors, elle grandit peu à peu.
        let rangee: Vec<f32> = (80 / PAS..=180 / PAS).rev().map(|i| s.en(i, 30)).collect();
        assert!(rangee.windows(2).all(|w| w[1] >= w[0]), "{rangee:?}");
        assert!(rangee.iter().any(|&v| v > 0.05 && v < 0.95), "{rangee:?}");
        assert_eq!(s.en(99, 99), 0.0, "hors de l'image");
    }

    #[test]
    fn le_retard_va_des_racines_aux_pointes() {
        // Des cheveux partout : au-dessus des racines, rien ; à mi-longueur, le retard du milieu ; au bas, celui des
        // pointes ; entre les deux, sans sauter.
        let (n, toute) = (20, egui::ColorImage::new([160, 160], vec![Color32::BLACK; 160 * 160]));
        let blanc = egui::ColorImage::new([160, 160], vec![Color32::WHITE; 160 * 160]);
        let s = Arc::new(Souplesse::de(&toute, &blanc));
        assert_eq!(s.noeuds.len(), n * n);
        let r = Retard::d_essai(s, -0.01, -0.03);
        let a = |v: f32| r.ecart(5, 5, v).x;
        assert_eq!((a(RACINES - 0.2), a(RACINES)), (0.0, 0.0));
        assert!((a(RACINES + LONGUEUR / 2.0) + 0.01).abs() < 1e-6 && (a(RACINES + LONGUEUR) + 0.03).abs() < 1e-6);
        let pas: Vec<f32> = (0..=40).map(|k| a(RACINES + LONGUEUR * k as f32 / 40.0)).collect();
        assert!(pas.windows(2).all(|w| w[1] <= w[0] && w[0] - w[1] < 0.003), "{pas:?}");
        assert_eq!(r.ecart(5, 5, 0.5).y, 0.0, "de côté seulement");
        assert_eq!(Retard::default().ecart(5, 5, 0.0), Vec2::ZERO, "sans masque, aucun");
    }
}
