//! Le flot optique du portrait d'Eve : où chaque point de la photo se retrouve dans une pose — la paupière qui descend,
//! la mâchoire qui s'ouvre, les sourcils qui montent —, de quoi peindre le passage d'une pose à l'autre comme un
//! mouvement plutôt que comme deux images l'une sur l'autre ([`super::rendu`]). La méthode de Horn et Schunck, du
//! grossier au fin, sur l'image réduite au quart, puis lissée : un champ doux, sans pli, que le maillage suit. Des essais
//! sur les poses de LivePortrait l'ont réglée : sans le lissage, la paupière fermée se déchirait ; au quart, le passage
//! d'une pose à l'autre est aussi juste qu'à la moitié, pour quatre fois moins de calcul.

/// Les réductions de moitié de l'image avant le calcul du flot : deux, au quart.
const REDUCTIONS: u32 = 2;
/// La régularité du champ (le α de Horn et Schunck, pour une luminance de 0 à 255) : plus grande, le champ se plie moins
/// aux détails.
const REGULARITE: f32 = 30.0;
/// Les déformations par étage de la pyramide (la pose redéformée par le flot trouvé, puis affinée), et les itérations
/// de chacune.
const DEFORMATIONS: usize = 3;
const ITERATIONS: usize = 60;
/// Le côté sous lequel la pyramide s'arrête : son étage le plus grossier.
const COTE_GROSSIER: usize = 32;
/// Le lissage final du champ (l'écart type d'un flou gaussien, en pixels de l'image réduite) : sur l'image entière, six
/// pixels — le maillage du portrait, un nœud tous les huit, le suit sans rien perdre de ce qui compte.
const LISSAGE: f32 = 1.5;

/// Une image d'un seul canal (la luminance, de 0 à 255), `l` sur `h`, rangée par rangée.
#[derive(Clone)]
pub(super) struct Gris {
    pub(super) l: usize,
    pub(super) h: usize,
    pub(super) v: Vec<f32>,
}

impl Gris {
    /// La valeur au point (x, y), interpolée entre ses quatre voisins ; hors de l'image, celle de son bord.
    fn en(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.l - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.l - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| self.v[y * self.l + x];
        let haut = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * fx;
        let bas = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * fx;
        haut + (bas - haut) * fy
    }

    /// L'image deux fois plus petite : un flou binomial (1, 4, 6, 4, 1), puis un pixel sur deux — le pixel `k` de la
    /// réduite est le pixel `2k` de l'image.
    fn reduite(&self) -> Gris {
        const NOYAU: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let (l, h) = (self.l, self.h);
        let mut rangees = vec![0.0; l * h];
        for y in 0..h {
            for x in 0..l {
                rangees[y * l + x] =
                    (0..5).map(|k| NOYAU[k] * self.v[y * l + (x + k).saturating_sub(2).min(l - 1)]).sum();
            }
        }
        let (l2, h2) = (l.div_ceil(2), h.div_ceil(2));
        let mut v = vec![0.0; l2 * h2];
        for y in 0..h2 {
            for x in 0..l2 {
                v[y * l2 + x] =
                    (0..5).map(|k| NOYAU[k] * rangees[(2 * y + k).saturating_sub(2).min(h - 1) * l + 2 * x]).sum();
            }
        }
        Gris { l: l2, h: h2, v }
    }
}

/// Un champ de déplacements, un par pixel, `l` sur `h` : (dx, dy), en pixels.
#[derive(Clone)]
struct Champ {
    l: usize,
    h: usize,
    v: Vec<[f32; 2]>,
}

impl Champ {
    fn nul(l: usize, h: usize) -> Champ {
        Champ { l, h, v: vec![[0.0; 2]; l * h] }
    }

    /// Le déplacement au point (x, y), interpolé entre ses quatre voisins ; hors du champ, celui de son bord.
    fn en(&self, x: f32, y: f32) -> [f32; 2] {
        let x = x.clamp(0.0, (self.l - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.l - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| self.v[y * self.l + x];
        let m = |a: [f32; 2], b: [f32; 2], f: f32| [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
        m(m(p(x0, y0), p(x1, y0), fx), m(p(x0, y1), p(x1, y1), fx), fy)
    }

    /// Le champ porté à l'étage plus fin de la pyramide (`l` sur `h`) : interpolé, et ses déplacements doublés.
    fn agrandi(&self, l: usize, h: usize) -> Champ {
        let v = (0..l * h)
            .map(|k| {
                let [dx, dy] = self.en((k % l) as f32 / 2.0, (k / l) as f32 / 2.0);
                [2.0 * dx, 2.0 * dy]
            })
            .collect();
        Champ { l, h, v }
    }

    /// Le champ passé au filtre médian de trois sur trois, composante par composante : ses déplacements isolés s'en vont,
    /// ses bords restent nets.
    fn median(&self) -> Champ {
        let (l, h) = (self.l, self.h);
        let mut v = self.v.clone();
        for y in 0..h {
            let rangees = [y.saturating_sub(1), y, (y + 1).min(h - 1)].map(|r| &self.v[r * l..r * l + l]);
            for x in 0..l {
                let colonnes = [x.saturating_sub(1), x, (x + 1).min(l - 1)];
                for c in 0..2 {
                    let mut neuf = [0.0f32; 9];
                    for (k, case) in neuf.iter_mut().enumerate() {
                        *case = rangees[k / 3][colonnes[k % 3]][c];
                    }
                    v[y * l + x][c] = mediane_de_neuf(neuf);
                }
            }
        }
        Champ { l, h, v }
    }

    /// Le champ adouci d'un flou gaussien d'écart type `sigma` (en pixels), les rangées puis les colonnes ; au bord, le
    /// bord se prolonge.
    fn lisse(&self, sigma: f32) -> Champ {
        let r = (3.0 * sigma).ceil() as usize;
        let noyau: Vec<f32> =
            (0..=2 * r).map(|k| (-((k as f32 - r as f32).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
        let total: f32 = noyau.iter().sum();
        let (l, h) = (self.l, self.h);
        let passe = |source: &[[f32; 2]], horizontale: bool| -> Vec<[f32; 2]> {
            (0..l * h)
                .map(|i| {
                    let (x, y) = (i % l, i / l);
                    let mut s = [0.0; 2];
                    for (k, &poids) in noyau.iter().enumerate() {
                        let j = if horizontale {
                            y * l + (x + k).saturating_sub(r).min(l - 1)
                        } else {
                            (y + k).saturating_sub(r).min(h - 1) * l + x
                        };
                        s[0] += poids * source[j][0];
                        s[1] += poids * source[j][1];
                    }
                    [s[0] / total, s[1] / total]
                })
                .collect()
        };
        let v = passe(&passe(&self.v, true), false);
        Champ { l, h, v }
    }
}

/// La médiane de neuf valeurs, par un réseau de dix-neuf échanges (celui de Paeth) : bien plus vite qu'un tri.
fn mediane_de_neuf(mut p: [f32; 9]) -> f32 {
    const ECHANGES: [(usize, usize); 19] = [
        (1, 2),
        (4, 5),
        (7, 8),
        (0, 1),
        (3, 4),
        (6, 7),
        (1, 2),
        (4, 5),
        (7, 8),
        (0, 3),
        (5, 8),
        (4, 7),
        (3, 6),
        (1, 4),
        (2, 5),
        (4, 7),
        (4, 2),
        (6, 4),
        (4, 2),
    ];
    for (a, b) in ECHANGES {
        (p[a], p[b]) = (p[a].min(p[b]), p[a].max(p[b]));
    }
    p[4]
}

/// Le flot d'une image vers une autre, calculé sur leurs réductions : où un point de la première se retrouve dans la
/// seconde, en pixels de l'image entière ([`Flot::en`]).
pub(super) struct Flot {
    champ: Champ,
    echelle: f32,
}

impl Flot {
    /// Le déplacement du point (x, y) de l'image entière (en pixels), interpolé.
    pub(super) fn en(&self, x: f32, y: f32) -> [f32; 2] {
        let [dx, dy] = self.champ.en(x / self.echelle, y / self.echelle);
        [dx * self.echelle, dy * self.echelle]
    }
}

/// Le flot de `a` vers `b` (deux images de même taille) : pour chaque point `x` de `a`, où il se retrouve dans `b` —
/// `a(x) ≈ b(x + F(x))` —, doux et sans pli ([`LISSAGE`]) ; calculé au quart ([`REDUCTIONS`]).
pub(super) fn flot(a: &Gris, b: &Gris) -> Flot {
    let (mut a, mut b) = (a.clone(), b.clone());
    for _ in 0..REDUCTIONS {
        (a, b) = (a.reduite(), b.reduite());
    }
    let mut etages = vec![(a, b)];
    while let Some((a, b)) = etages.last().filter(|(a, _)| a.l.min(a.h) > COTE_GROSSIER) {
        let suivant = (a.reduite(), b.reduite());
        etages.push(suivant);
    }
    let mut f: Option<Champ> = None;
    for (a, b) in etages.iter().rev() {
        let mut champ = match f {
            Some(f) => f.agrandi(a.l, a.h),
            None => Champ::nul(a.l, a.h),
        };
        for _ in 0..DEFORMATIONS {
            affiner(a, b, &mut champ);
            champ = champ.median();
        }
        f = Some(champ);
    }
    let champ = f.map_or_else(|| Champ::nul(1, 1), |f| f.lisse(LISSAGE));
    Flot { champ, echelle: (1u32 << REDUCTIONS) as f32 }
}

/// Une déformation de Horn et Schunck : `b` redéformé par le champ `f`, l'écart qui reste à `a` et ses dérivées, puis le
/// champ affiné, de proche en proche (chaque point vers la moyenne de ses voisins, corrigée de ce que l'image dit).
fn affiner(a: &Gris, b: &Gris, f: &mut Champ) {
    let (l, h) = (a.l, a.h);
    let deforme: Vec<f32> = (0..l * h)
        .map(|i| {
            let [dx, dy] = f.v[i];
            b.en((i % l) as f32 + dx, (i / l) as f32 + dy)
        })
        .collect();
    let milieu = |i: usize| (a.v[i] + deforme[i]) / 2.0;
    // Pour chaque point : les dérivées de l'image (ix, iy), ce que l'écart dit du flot de départ — l'écart, moins ce que
    // le flot de départ en explique déjà —, et l'inverse du dénominateur de Horn et Schunck.
    let alpha2 = REGULARITE * REGULARITE;
    let coef: Vec<[f32; 4]> = (0..l * h)
        .map(|i| {
            let (x, y) = (i % l, i / l);
            let ix = if x > 0 && x + 1 < l { (milieu(i + 1) - milieu(i - 1)) / 2.0 } else { 0.0 };
            let iy = if y > 0 && y + 1 < h { (milieu(i + l) - milieu(i - l)) / 2.0 } else { 0.0 };
            let [fx, fy] = f.v[i];
            [ix, iy, deforme[i] - a.v[i] - ix * fx - iy * fy, 1.0 / (alpha2 + ix * ix + iy * iy)]
        })
        .collect();
    let mut g = f.v.clone();
    let mut suivant = vec![[0.0f32; 2]; l * h];
    for _ in 0..ITERATIONS {
        for y in 0..h {
            let (haut, ici, bas) = (y.saturating_sub(1) * l, y * l, (y + 1).min(h - 1) * l);
            let (rh, ri, rb) = (&g[haut..haut + l], &g[ici..ici + l], &g[bas..bas + l]);
            let (co, sortie) = (&coef[ici..ici + l], &mut suivant[ici..ici + l]);
            for x in 0..l {
                let (gauche, droite) = (ri[x.saturating_sub(1)], ri[(x + 1).min(l - 1)]);
                let mx = (rh[x][0] + rb[x][0] + gauche[0] + droite[0]) * 0.25;
                let my = (rh[x][1] + rb[x][1] + gauche[1] + droite[1]) * 0.25;
                let [ix, iy, c, inverse] = co[x];
                let t = (ix * mx + iy * my + c) * inverse;
                sortie[x] = [mx - ix * t, my - iy * t];
            }
        }
        std::mem::swap(&mut g, &mut suivant);
    }
    f.v = g;
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Une image texturée, douce, `l` sur `h`, décalée de (dx, dy) : la valeur de l'image d'origine en (x − dx, y − dy).
    fn motif(l: usize, h: usize, dx: f32, dy: f32) -> Gris {
        let v = (0..l * h)
            .map(|i| {
                let (x, y) = ((i % l) as f32 - dx, (i / l) as f32 - dy);
                128.0
                    + 50.0 * (x * 0.09 + y * 0.03).sin()
                    + 40.0 * (y * 0.08 - x * 0.025).cos()
                    + 25.0 * (x * 0.05 + y * 0.06).sin() * (x * 0.015).cos()
            })
            .collect();
        Gris { l, h, v }
    }

    #[test]
    fn le_flot_retrouve_un_decalage() {
        // La pose : la photo décalée de 3,5 pixels vers la droite et 2 vers le haut ; loin des bords, le flot le
        // retrouve à moins d'un demi-pixel près (il se calcule au quart).
        let (a, b) = (motif(192, 160, 0.0, 0.0), motif(192, 160, 3.5, -2.0));
        let f = flot(&a, &b);
        for (x, y) in [(96.0, 80.0), (60.0, 60.0), (130.0, 100.0)] {
            let [dx, dy] = f.en(x, y);
            assert!((dx - 3.5).abs() < 0.5 && (dy + 2.0).abs() < 0.5, "en ({x}, {y}) : {dx} {dy}");
        }
        // Deux images pareilles : rien ne bouge.
        let nul = flot(&a, &a);
        assert!(nul.champ.v.iter().all(|[dx, dy]| dx.abs() < 1e-3 && dy.abs() < 1e-3));
    }

    #[test]
    fn une_image_se_reduit_et_un_champ_s_interpole() {
        // Réduite, une image garde sa moyenne ; son pixel k est le pixel 2k de l'image.
        let a = motif(41, 30, 0.0, 0.0);
        let r = a.reduite();
        assert_eq!((r.l, r.h), (21, 15));
        assert!((r.v[7 * 21 + 10] - a.v[14 * 41 + 20]).abs() < 12.0);
        // Un champ s'interpole entre ses points, et garde son bord au-delà.
        let c = Champ { l: 2, h: 2, v: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 4.0], [2.0, 4.0]] };
        assert_eq!(c.en(0.5, 0.5), [1.0, 2.0]);
        assert_eq!(c.en(-3.0, 9.0), [0.0, 4.0]);
        // Lissé, un champ uniforme ne change pas.
        let u = Champ { l: 9, h: 7, v: vec![[1.5, -0.5]; 63] }.lisse(2.0);
        assert!(u.v.iter().all(|[dx, dy]| (dx - 1.5).abs() < 1e-5 && (dy + 0.5).abs() < 1e-5));
        // La médiane de neuf, dans tous les ordres.
        assert_eq!(mediane_de_neuf([9.0, 1.0, 8.0, 2.0, 7.0, 3.0, 6.0, 4.0, 5.0]), 5.0);
        assert_eq!(mediane_de_neuf([1.0, 1.0, 1.0, 9.0, 9.0, 9.0, 9.0, 1.0, 5.0]), 5.0);
        assert_eq!(mediane_de_neuf([0.0, -1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0]), -4.0);
    }
}
