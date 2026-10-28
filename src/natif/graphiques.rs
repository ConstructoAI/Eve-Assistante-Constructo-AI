//! Les graphiques des cartes épinglées au tableau de bord, à la manière d'un Power BI : des colonnes (côte à côte ou
//! empilées), des barres horizontales, des lignes, des aires, un anneau — d'après la compétence de visualisation des
//! données. Des marques fines : une colonne de 24 points au plus, son bout arrondi de 4 points et carré sur la ligne de
//! base ; une ligne de 2 points ; un point de 8 points cerclé de blanc ; une aire lavée à 10 %. Deux points de blanc
//! entre deux marques qui se touchent (les parts d'une pile, deux colonnes voisines, les parts d'un anneau). Une grille
//! de filets pleins et pâles, la ligne de base un ton plus foncé. Une légende dès deux séries (aucune pour une seule :
//! le titre de la carte la nomme) ; des valeurs écrites avec parcimonie — la plus haute et la dernière d'une série seule,
//! le bout de chaque ligne quand elles sont quatre au plus et ne se chevauchent pas, chaque barre horizontale (c'est une
//! liste) ; les autres dans la bulle du survol et dans la vue en tableau de la carte. Au survol, la bande d'une colonne,
//! la rangée d'une barre ou la part d'un anneau répond ; sur des lignes, le réticule trouve l'abscisse la plus proche et
//! la bulle dit toutes les séries, la valeur d'abord. Les couleurs : la palette catégorielle de la compétence, dans son
//! ordre, jamais recyclée (huit séries au plus ; une série soulignée, les autres en gris) ; le texte garde l'encre du
//! texte, jamais celle d'une série.

use std::f32::consts::TAU;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2,
};
use serde_json::{Value, json};

use super::SEMI;
use super::theme::{self, erp};

/// La palette catégorielle de la compétence de visualisation des données (son thème clair), dans son ordre — bleu,
/// orange, aqua, jaune, magenta, vert, violet, rouge —, qui la rend sûre pour qui voit mal les couleurs : validée par
/// son script sur le blanc des cartes (le 28 octobre 2026 : toutes ses vérifications passent ; l'aqua, le jaune et le
/// magenta sous 3:1 de contraste, d'où la légende, les valeurs écrites et la vue en tableau).
pub(crate) const SERIES: [Color32; 8] = [
    Color32::from_rgb(0x2a, 0x78, 0xd6),
    Color32::from_rgb(0xeb, 0x68, 0x34),
    Color32::from_rgb(0x1b, 0xaf, 0x7a),
    Color32::from_rgb(0xed, 0xa1, 0x00),
    Color32::from_rgb(0xe8, 0x7b, 0xa4),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x62, 0x50, 0xd6),
    Color32::from_rgb(0xe3, 0x49, 0x48),
];
/// Une série de contexte (une autre est soulignée), la part « Autres » d'un anneau : le gris des tableaux de bord.
const CONTEXTE: Color32 = erp::GRIS.pastel;
/// La grille (un ton sous la surface) et la ligne de base (un ton de plus), en filets pleins.
const GRILLE: Color32 = Color32::from_rgb(0xE1, 0xE0, 0xD9);
const BASE: Color32 = Color32::from_rgb(0xC3, 0xC2, 0xB7);
/// La colonne la plus épaisse ; le blanc entre deux marques qui se touchent ; la hauteur du tracé ; les parts d'un
/// anneau ; les catégories d'un graphique.
const EPAISSEUR_MAX: f32 = 24.0;
const INTERVALLE: f32 = 2.0;
const HAUTEUR: f32 = 170.0;
const PARTS_MAX: usize = 6;
const CATEGORIES_MAX: usize = 60;

/// La forme d'un graphique : son sens. L'évolution dans le temps en lignes ou en aires, une comparaison en colonnes, des
/// noms longs en barres horizontales, les parts de chaque catégorie en colonnes empilées, les parts d'un tout en anneau.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Forme {
    Colonnes,
    Empilees,
    Horizontales,
    Lignes,
    Aires,
    Anneau,
}

/// Une série : son nom (vide pour une série seule), une valeur par étiquette (`None` : elle manque).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Serie {
    pub(crate) nom: String,
    pub(crate) valeurs: Vec<Option<f64>>,
}

/// Un graphique lu d'une carte épinglée : sa forme, ses étiquettes (l'axe, ou les parts d'un anneau), ses séries, leur
/// unité, la série qu'il souligne ; un anneau dont la dernière part réunit les autres.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Graphique {
    pub(crate) forme: Forme,
    pub(crate) etiquettes: Vec<String>,
    pub(crate) series: Vec<Serie>,
    pub(crate) unite: String,
    pub(crate) souligne: Option<usize>,
    pub(crate) autres: bool,
}

impl Graphique {
    /// Le graphique d'une carte épinglée : sa `forme`, ses `etiquettes`, ses `series` (chacune son `nom` et ses
    /// `valeurs` ; une liste de nombres seule, une série sans nom ; ou des `valeurs` à même la carte), son `unite`, la
    /// série qu'il `souligne`. Une valeur s'écrit en nombre ou en texte (« 12,5 ») ; vide ou fausse, elle manque. Huit
    /// séries, soixante étiquettes au plus ; un anneau, six parts au plus (le reste réuni en « Autres »). Ce qui ne se
    /// dessine pas dit pourquoi.
    pub(crate) fn de(c: &Value) -> Result<Graphique, String> {
        let forme = match eve::parole::normaliser(c["forme"].as_str().unwrap_or("")).as_str() {
            "" | "barres" | "colonnes" | "histogramme" => Forme::Colonnes,
            "empilees" | "barres empilees" | "colonnes empilees" | "pile" => Forme::Empilees,
            "horizontales" | "barres horizontales" => Forme::Horizontales,
            "lignes" | "ligne" | "courbe" | "courbes" => Forme::Lignes,
            "aires" | "aire" => Forme::Aires,
            "anneau" | "beigne" | "tarte" | "camembert" | "secteurs" => Forme::Anneau,
            _ => {
                let forme = c["forme"].as_str().unwrap_or("");
                return Err(format!("Eve ne dessine pas encore un graphique « {forme} »."));
            }
        };
        let mut series: Vec<Serie> = match &c["series"] {
            Value::Array(l) => l
                .iter()
                .map(|s| match s {
                    Value::Array(v) => Serie { nom: String::new(), valeurs: v.iter().map(valeur).collect() },
                    s => Serie {
                        nom: en_texte(&s["nom"]).trim().to_string(),
                        valeurs: s["valeurs"].as_array().map(|v| v.iter().map(valeur).collect()).unwrap_or_default(),
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        if series.is_empty()
            && let Some(v) = c["valeurs"].as_array()
        {
            series.push(Serie { nom: String::new(), valeurs: v.iter().map(valeur).collect() });
        }
        series.truncate(SERIES.len());
        let mut etiquettes: Vec<String> =
            c["etiquettes"].as_array().map(|l| l.iter().map(en_texte).collect()).unwrap_or_default();
        let n = if etiquettes.is_empty() {
            series.iter().map(|s| s.valeurs.len()).max().unwrap_or(0)
        } else {
            etiquettes.len()
        };
        let n = n.min(CATEGORIES_MAX);
        etiquettes.truncate(n);
        if etiquettes.is_empty() {
            etiquettes = (1..=n).map(|k| k.to_string()).collect();
        }
        for s in &mut series {
            s.valeurs.resize(n, None);
        }
        if n == 0 || series.iter().all(|s| s.valeurs.iter().all(Option::is_none)) {
            return Err("Un graphique sans valeurs.".into());
        }
        let souligne = c["souligne"]
            .as_str()
            .map(eve::parole::normaliser)
            .and_then(|nom| series.iter().position(|s| !s.nom.is_empty() && eve::parole::normaliser(&s.nom) == nom));
        let unite = en_texte(&c["unite"]).trim().to_string();
        let mut g = Graphique { forme, etiquettes, series, unite, souligne, autres: false };
        if forme == Forme::Anneau {
            g.plier_les_parts();
            if g.etiquettes.is_empty() {
                return Err("Un anneau sans part plus grande que zéro.".into());
            }
        }
        Ok(g)
    }

    /// Un anneau : la première série, ses parts plus grandes que zéro, six au plus — la sixième réunit les autres.
    fn plier_les_parts(&mut self) {
        let Some(premiere) = self.series.first() else { return };
        let mut parts: Vec<(String, f64)> = self
            .etiquettes
            .iter()
            .cloned()
            .zip(premiere.valeurs.iter().copied())
            .filter_map(|(e, v)| v.filter(|v| *v > 0.0).map(|v| (e, v)))
            .collect();
        if parts.len() > PARTS_MAX {
            let autres: f64 = parts[PARTS_MAX - 1..].iter().map(|(_, v)| v).sum();
            parts.truncate(PARTS_MAX - 1);
            parts.push(("Autres".into(), autres));
            self.autres = true;
        }
        let nom = premiere.nom.clone();
        self.etiquettes = parts.iter().map(|(e, _)| e.clone()).collect();
        self.series = vec![Serie { nom, valeurs: parts.iter().map(|(_, v)| Some(*v)).collect() }];
        self.souligne = None;
    }

    /// La couleur de la série `k` (d'un anneau : de sa part `k`) : son rang dans la palette ; soulignée, le premier, et
    /// les autres en gris ; la part « Autres », en gris.
    fn couleur(&self, k: usize) -> Color32 {
        if self.forme == Forme::Anneau && self.autres && k + 1 == self.etiquettes.len() {
            return CONTEXTE;
        }
        match self.souligne {
            Some(s) if s != k => CONTEXTE,
            Some(_) => SERIES[0],
            None => SERIES[k % SERIES.len()],
        }
    }

    /// Le nom de la série `k` tel qu'il se lit (une série sans nom : « Valeur », ou « Série 2 » parmi d'autres).
    fn nom(&self, k: usize) -> String {
        match &self.series[k].nom {
            n if !n.is_empty() => n.clone(),
            _ if self.series.len() == 1 => "Valeur".into(),
            _ => format!("Série {}", k + 1),
        }
    }

    /// Une valeur dite en entier, avec son unité (« 12 500 $ »).
    fn dite(&self, v: f64) -> String {
        avec_unite(&nombre_fr(v), &self.unite)
    }

    /// Le graphique en tableau — la vue en tableau de sa carte, où chaque valeur se lit sans survol : une rangée par
    /// étiquette, une colonne par série ; un anneau, ses parts, leur valeur et leur part du total. Une carte « tableau »
    /// en JSON (`cartes::Cartes::tableau_seul`).
    pub(crate) fn en_tableau(&self) -> String {
        let dite = |v: Option<f64>| v.map_or(String::new(), |v| self.dite(v));
        if self.forme == Forme::Anneau {
            let valeurs = &self.series[0].valeurs;
            let total: f64 = valeurs.iter().flatten().sum();
            let lignes: Vec<Value> = self
                .etiquettes
                .iter()
                .zip(valeurs)
                .map(|(e, v)| json!([e, dite(*v), pourcent(v.unwrap_or(0.0) / total)]))
                .collect();
            let pied = ["Total".to_string(), dite(Some(total)), pourcent(1.0)];
            return json!({ "colonnes": ["", "Valeur", "Part"], "lignes": lignes, "pied": pied }).to_string();
        }
        let colonnes: Vec<String> =
            std::iter::once(String::new()).chain((0..self.series.len()).map(|k| self.nom(k))).collect();
        let lignes: Vec<Value> = self
            .etiquettes
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let rangee: Vec<String> =
                    std::iter::once(e.clone()).chain(self.series.iter().map(|s| dite(s.valeurs[i]))).collect();
                json!(rangee)
            })
            .collect();
        json!({ "colonnes": colonnes, "lignes": lignes }).to_string()
    }

    /// L'étendue de l'axe des valeurs : zéro compris ; des colonnes empilées, la plus haute pile.
    fn etendue(&self) -> (f64, f64) {
        let toutes = self.series.iter().flat_map(|s| s.valeurs.iter().flatten().copied());
        let (mut bas, mut haut) = toutes.fold((0.0_f64, 0.0_f64), |(b, h), v| (b.min(v), h.max(v)));
        if self.forme == Forme::Empilees {
            bas = 0.0;
            haut = (0..self.etiquettes.len())
                .map(|i| self.series.iter().filter_map(|s| s.valeurs[i]).filter(|v| *v > 0.0).sum::<f64>())
                .fold(0.0, f64::max);
        }
        (bas, haut)
    }
}

/// Une valeur d'une série : un nombre, ou un texte qui en est un (« 12,5 », « 1 250 ») ; autrement, elle manque.
fn valeur(v: &Value) -> Option<f64> {
    let lue = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace([' ', '\u{a0}', '\u{202f}'], "").replace(',', ".").parse().ok(),
        _ => None,
    };
    lue.filter(|v| v.is_finite())
}

/// Un texte d'une carte : une chaîne telle quelle, un nombre tel qu'écrit.
fn en_texte(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

/// Un nombre à la française : l'espace fine entre les milliers, la virgule, deux décimales au plus (sans zéro de trop).
pub(crate) fn nombre_fr(v: f64) -> String {
    let arrondi = (v * 100.0).round() / 100.0;
    let signe = if arrondi < 0.0 { "-" } else { "" };
    let a = arrondi.abs();
    let entier = crate::centre::milliers(a.trunc());
    let fraction = format!("{:.2}", a.fract());
    let fraction = fraction.trim_start_matches('0').trim_start_matches('.').trim_end_matches('0');
    if fraction.is_empty() { format!("{signe}{entier}") } else { format!("{signe}{entier},{fraction}") }
}

/// Un texte et son unité, séparés d'une espace insécable (« 45 % », « 12 500 $ ») ; sans unité, le texte.
fn avec_unite(texte: &str, unite: &str) -> String {
    if unite.is_empty() { texte.to_string() } else { format!("{texte}\u{a0}{unite}") }
}

/// Une part du total, en pour cent arrondi (« 34 % » ; « moins de 1 % » plutôt qu'un zéro).
fn pourcent(part: f64) -> String {
    match (part * 100.0).round() {
        x if x < 1.0 && part > 0.0 => "moins de 1\u{a0}%".into(),
        x => format!("{}\u{a0}%", nombre_fr(x)),
    }
}

/// Un nombre compact, pour un axe ou une étiquette : tel quel sous dix mille, puis « 12,5 k », « 1,2 M », « 3 G » — avec
/// son unité (« 12,5 k$ », « 45 % », « 38 h »).
pub(crate) fn compact(v: f64, unite: &str) -> String {
    let (n, multiple) = match v.abs() {
        a if a >= 1e9 => (v / 1e9, "G"),
        a if a >= 1e6 => (v / 1e6, "M"),
        a if a >= 1e4 => (v / 1e3, "k"),
        _ => (v, ""),
    };
    let n = if multiple.is_empty() { nombre_fr(n) } else { nombre_fr((n * 10.0).round() / 10.0) };
    let suffixe = format!("{multiple}{unite}");
    if suffixe.is_empty() { n } else { format!("{n}\u{a0}{suffixe}") }
}

/// Les graduations d'un axe de `bas` à `haut` : des nombres ronds (un pas de 1, 2, 2,5 ou 5 fois une puissance de dix),
/// cinq ou six, zéro compris quand l'axe le traverse.
pub(crate) fn graduer(bas: f64, haut: f64) -> Vec<f64> {
    let (bas, haut) = if haut - bas < 1e-9 { (bas.min(0.0), bas.max(0.0) + 1.0) } else { (bas, haut) };
    let etendue = haut - bas;
    let puissance = 10f64.powf((etendue / 4.0).log10().floor());
    let pas = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * puissance)
        .find(|p| etendue / p <= 5.0)
        .unwrap_or(10.0 * puissance);
    // Une marge pour l'arrondi : 1,9999999 vaut 2 ; zéro reste zéro.
    let (debut, fin) = ((bas / pas + 1e-9).floor() as i64, (haut / pas - 1e-9).ceil() as i64);
    (debut..=fin).map(|k| if k == 0 { 0.0 } else { k as f64 * pas }).collect()
}

/// Le graphique `g` dans toute la largeur donnée ; `cle` : de quoi nommer sa bulle.
pub(crate) fn graphique(ui: &mut egui::Ui, g: &Graphique, cle: &str) {
    match g.forme {
        Forme::Anneau => anneau(ui, g, cle),
        Forme::Horizontales => horizontales(ui, g, cle),
        _ => vertical(ui, g, cle),
    }
}

/// La police des axes et des étiquettes ; celle d'une valeur écrite.
fn police_d_axe() -> FontId {
    FontId::proportional(11.5)
}

fn police_de_valeur() -> FontId {
    FontId::new(12.0, FontFamily::Name(SEMI.into()))
}

/// Des colonnes (côte à côte ou empilées), des lignes ou des aires : l'axe des valeurs à gauche, ses graduations et leur
/// grille ; l'axe des étiquettes dessous, une sur deux ou sur trois s'il le faut pour qu'elles ne se touchent pas.
fn vertical(ui: &mut egui::Ui, g: &Graphique, cle: &str) {
    if g.series.len() > 1 {
        legende(ui, g);
        ui.add_space(6.0);
    }
    let n = g.etiquettes.len();
    let (bas, haut) = g.etendue();
    let graduations = graduer(bas, haut);
    let (min, max) = (graduations[0], graduations[graduations.len() - 1]);
    let p = ui.painter().clone();
    let marques: Vec<Arc<egui::Galley>> =
        graduations.iter().map(|v| p.layout_no_wrap(compact(*v, &g.unite), police_d_axe(), erp::TEXTE_2)).collect();
    let gauche = marques.iter().map(|m| m.size().x).fold(0.0, f32::max) + 8.0;
    let courbes = matches!(g.forme, Forme::Lignes | Forme::Aires);
    // Le bout de chaque ligne, écrit à sa droite (quatre séries au plus) : la place qu'il prend.
    let bouts: Vec<Option<(usize, f64)>> =
        g.series.iter().map(|s| s.valeurs.iter().enumerate().rev().find_map(|(i, v)| v.map(|v| (i, v)))).collect();
    let ecrits_au_bout = courbes && g.series.len() <= 4;
    let l_bout = if ecrits_au_bout {
        bouts
            .iter()
            .flatten()
            .map(|(_, v)| p.layout_no_wrap(compact(*v, &g.unite), police_de_valeur(), erp::TITRE).size().x)
            .fold(0.0, f32::max)
            + 10.0
    } else {
        6.0
    };
    let largeur = ui.available_width();
    // Au-dessus du tracé, la place d'une valeur écrite sur la plus haute colonne.
    let (rect, rep) = ui.allocate_exact_size(vec2(largeur, HAUTEUR + 40.0), Sense::hover());
    let trace = Rect::from_min_max(
        pos2(rect.left() + gauche, rect.top() + 20.0),
        pos2((rect.right() - l_bout).max(rect.left() + gauche + 40.0), rect.top() + 20.0 + HAUTEUR),
    );
    let y = |v: f64| trace.bottom() - ((v - min) / (max - min)) as f32 * trace.height();
    for (v, m) in graduations.iter().zip(marques) {
        let yy = y(*v);
        p.hline(trace.x_range(), yy, Stroke::new(1.0, if *v == 0.0 { BASE } else { GRILLE }));
        p.galley(pos2(trace.left() - 8.0 - m.size().x, yy - m.size().y / 2.0), m, erp::TEXTE_2);
    }
    let bande = trace.width() / n as f32;
    let x_de = |i: usize| -> f32 {
        match (courbes, n) {
            (true, 1) => trace.center().x,
            (true, _) => trace.left() + 4.0 + (trace.width() - 8.0) * i as f32 / (n - 1) as f32,
            (false, _) => trace.left() + bande * (i as f32 + 0.5),
        }
    };
    // L'axe des étiquettes.
    let etiquettes: Vec<Arc<egui::Galley>> =
        g.etiquettes.iter().map(|e| p.layout_no_wrap(e.clone(), police_d_axe(), erp::TEXTE_2)).collect();
    let plus_large = etiquettes.iter().map(|e| e.size().x).fold(0.0, f32::max) + 10.0;
    let espace = if courbes && n > 1 { (trace.width() - 8.0) / (n - 1) as f32 } else { bande };
    let une_sur = ((plus_large / espace.max(1.0)).ceil() as usize).max(1);
    for (i, e) in etiquettes.into_iter().enumerate().filter(|(i, _)| i % une_sur == 0) {
        let x = x_de(i).clamp(rect.left() + e.size().x / 2.0, rect.right() - e.size().x / 2.0);
        p.galley(pos2(x - e.size().x / 2.0, trace.bottom() + 7.0), e, erp::TEXTE_2);
    }
    // Le survol : la bande d'une colonne, ou l'abscisse la plus proche d'une ligne.
    let pointeur = rep.hover_pos().filter(|pt| trace.expand2(vec2(4.0, 10.0)).contains(*pt));
    let survol = pointeur.map(|pt| {
        if courbes {
            (0..n).min_by(|a, b| (x_de(*a) - pt.x).abs().total_cmp(&(x_de(*b) - pt.x).abs())).unwrap_or(0)
        } else {
            (((pt.x - trace.left()) / bande).floor().max(0.0) as usize).min(n - 1)
        }
    });
    if courbes {
        tracer_les_courbes(&p, g, trace, &x_de, &y, min, survol);
    } else {
        tracer_les_colonnes(&p, g, trace, bande, &y, min, survol);
    }
    // Les valeurs écrites : d'une série seule, la plus haute et la dernière ; le bout de chaque ligne.
    if g.series.len() == 1 && g.forme != Forme::Empilees {
        let s = &g.series[0];
        let plus_haute =
            s.valeurs.iter().enumerate().filter_map(|(i, v)| v.map(|v| (i, v))).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, v)) = plus_haute
            && bouts[0].is_none_or(|(j, _)| j != i || !ecrits_au_bout)
        {
            valeur_au_dessus(&p, pos2(x_de(i), y(v.max(0.0))), &compact(v, &g.unite));
        }
        if !courbes
            && let Some((j, v)) = bouts[0]
            && plus_haute.is_none_or(|(i, _)| i != j)
        {
            valeur_au_dessus(&p, pos2(x_de(j), y(v.max(0.0))), &compact(v, &g.unite));
        }
    }
    if ecrits_au_bout {
        let mut places: Vec<(f32, String)> =
            bouts.iter().flatten().map(|(_, v)| (y(*v), compact(*v, &g.unite))).collect();
        places.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Des bouts qui se chevaucheraient ne s'écrivent pas : la légende et la bulle les disent.
        if places.windows(2).all(|w| w[1].0 - w[0].0 >= 14.0) {
            let x = trace.right() + 8.0;
            for (yy, texte) in places {
                let g = p.layout_no_wrap(texte, police_de_valeur(), erp::TITRE);
                p.galley(pos2(x, yy - g.size().y / 2.0), g, erp::TITRE);
            }
        }
    }
    if let (Some(i), Some(pt)) = (survol, pointeur) {
        let lignes: Vec<(Color32, String, String)> = (0..g.series.len())
            .filter_map(|k| {
                g.series[k].valeurs[i]
                    .map(|v| (g.couleur(k), g.dite(v), if g.series.len() > 1 { g.nom(k) } else { String::new() }))
            })
            .collect();
        // Une pile dit aussi son total.
        let titre = match g.forme {
            Forme::Empilees => {
                let total: f64 = g.series.iter().filter_map(|s| s.valeurs[i]).filter(|v| *v > 0.0).sum();
                format!("{} · {} au total", g.etiquettes[i], compact(total, &g.unite))
            }
            _ => g.etiquettes[i].clone(),
        };
        bulle(ui, pt, &titre, &lignes, cle);
    }
}

/// Une valeur écrite au-dessus d'une marque (`haut` : le haut de la marque), dans l'encre des valeurs.
fn valeur_au_dessus(p: &egui::Painter, haut: Pos2, texte: &str) {
    let g = p.layout_no_wrap(texte.to_string(), police_de_valeur(), erp::TITRE);
    let coin = pos2(haut.x - g.size().x / 2.0, haut.y - g.size().y - 3.0);
    p.galley(coin, g, erp::TITRE);
}

/// Le bout arrondi d'une colonne, 4 points au plus (moins pour une colonne étroite ou courte), carré sur la ligne de
/// base : en haut pour une valeur positive, en bas pour une négative.
fn bout(largeur: f32, hauteur: f32, vers_le_haut: bool) -> CornerRadius {
    let r = 4.0_f32.min(largeur / 2.0).min(hauteur).floor() as u8;
    if vers_le_haut { CornerRadius { nw: r, ne: r, sw: 0, se: 0 } } else { CornerRadius { nw: 0, ne: 0, sw: r, se: r } }
}

/// Les colonnes : côte à côte (deux points de blanc entre elles), ou empilées (deux points de blanc entre leurs parts,
/// le bout arrondi en haut de la pile) ; la bande survolée lavée de gris derrière elles.
fn tracer_les_colonnes(
    p: &egui::Painter,
    g: &Graphique,
    trace: Rect,
    bande: f32,
    y: &dyn Fn(f64) -> f32,
    min: f64,
    survol: Option<usize>,
) {
    let (n, k) = (g.etiquettes.len(), g.series.len());
    if let Some(i) = survol {
        let x = trace.left() + bande * i as f32;
        p.rect_filled(Rect::from_x_y_ranges(x..=x + bande, trace.y_range()), 6.0, erp::SURVOL);
    }
    let zero = y(0.0_f64.max(min));
    if g.forme == Forme::Empilees {
        let epaisseur = (bande * 0.6).clamp(2.0, EPAISSEUR_MAX);
        for i in 0..n {
            let cx = trace.left() + bande * (i as f32 + 0.5);
            let sommet = (0..k).rev().find(|s| g.series[*s].valeurs[i].is_some_and(|v| v > 0.0));
            let mut cumul = 0.0;
            for s in 0..k {
                let Some(v) = g.series[s].valeurs[i].filter(|v| *v > 0.0) else { continue };
                let (dessous, dessus) = (y(cumul), y(cumul + v));
                let dessous = if cumul > 0.0 { dessous - INTERVALLE } else { dessous };
                cumul += v;
                if dessous - dessus < 0.5 {
                    continue;
                }
                let r = Rect::from_x_y_ranges(cx - epaisseur / 2.0..=cx + epaisseur / 2.0, dessus..=dessous);
                let rayon =
                    if sommet == Some(s) { bout(epaisseur, dessous - dessus, true) } else { CornerRadius::ZERO };
                p.rect_filled(r, rayon, g.couleur(s));
            }
        }
        return;
    }
    let intervalles = INTERVALLE * (k - 1) as f32;
    let groupe = (bande * 0.72).min(EPAISSEUR_MAX * k as f32 + intervalles);
    let barre = ((groupe - intervalles) / k as f32).clamp(1.0, EPAISSEUR_MAX);
    let groupe = barre * k as f32 + intervalles;
    for i in 0..n {
        let cx = trace.left() + bande * (i as f32 + 0.5);
        for s in 0..k {
            let Some(v) = g.series[s].valeurs[i] else { continue };
            let x0 = cx - groupe / 2.0 + s as f32 * (barre + INTERVALLE);
            let (a, b) = (zero, y(v));
            let (dessus, dessous) = (a.min(b), a.max(b));
            if dessous - dessus < 0.5 {
                continue;
            }
            let r = Rect::from_x_y_ranges(x0..=x0 + barre, dessus..=dessous);
            p.rect_filled(r, bout(barre, dessous - dessus, v >= 0.0), g.couleur(s));
        }
    }
}

/// Les lignes (2 points) et les aires (lavées à 10 % sous elles), la première série par-dessus ; une valeur qui manque
/// coupe la ligne ; le dernier point de chaque série, cerclé de blanc ; au survol, le réticule et un point sur chaque
/// série.
fn tracer_les_courbes(
    p: &egui::Painter,
    g: &Graphique,
    trace: Rect,
    x_de: &dyn Fn(usize) -> f32,
    y: &dyn Fn(f64) -> f32,
    min: f64,
    survol: Option<usize>,
) {
    let base = y(0.0_f64.max(min));
    for s in (0..g.series.len()).rev() {
        let couleur = g.couleur(s);
        // Les morceaux de la série : une valeur qui manque en commence un autre.
        let mut morceaux: Vec<Vec<Pos2>> = Vec::new();
        let mut en_cours: Vec<Pos2> = Vec::new();
        for (i, v) in g.series[s].valeurs.iter().enumerate() {
            match v {
                Some(v) => en_cours.push(pos2(x_de(i), y(*v))),
                None if !en_cours.is_empty() => morceaux.push(std::mem::take(&mut en_cours)),
                None => {}
            }
        }
        if !en_cours.is_empty() {
            morceaux.push(en_cours);
        }
        for m in &morceaux {
            if g.forme == Forme::Aires && m.len() > 1 {
                let lavis = couleur.gamma_multiply(0.10);
                let mut maille = Mesh::default();
                for w in m.windows(2) {
                    let k = maille.vertices.len() as u32;
                    for q in [w[0], w[1], pos2(w[1].x, base), pos2(w[0].x, base)] {
                        maille.colored_vertex(q, lavis);
                    }
                    maille.add_triangle(k, k + 1, k + 2);
                    maille.add_triangle(k, k + 2, k + 3);
                }
                p.add(Shape::mesh(maille));
            }
            if m.len() == 1 {
                p.circle_filled(m[0], 2.5, couleur);
            } else {
                p.add(Shape::line(m.clone(), Stroke::new(2.0, couleur)));
            }
        }
        if let Some(fin) = morceaux.last().and_then(|m| m.last()) {
            point(p, *fin, couleur);
        }
    }
    if let Some(i) = survol {
        let x = x_de(i);
        p.vline(x, trace.y_range(), Stroke::new(1.0, BASE));
        for s in 0..g.series.len() {
            if let Some(v) = g.series[s].valeurs[i] {
                point(p, pos2(x, y(v)), g.couleur(s));
            }
        }
    }
}

/// Un point de 8 points, cerclé de 2 points de blanc (il se lit où il croise une ligne).
fn point(p: &egui::Painter, c: Pos2, couleur: Color32) {
    p.circle_filled(c, 6.0, Color32::WHITE);
    p.circle_filled(c, 4.0, couleur);
}

/// Des barres horizontales, une rangée par étiquette (son nom à gauche, coupé s'il est trop long ; ses barres ; la valeur
/// au bout de chacune) ; la rangée survolée lavée de gris, et sa bulle quand il y a plusieurs séries.
fn horizontales(ui: &mut egui::Ui, g: &Graphique, cle: &str) {
    let k = g.series.len();
    if k > 1 {
        legende(ui, g);
        ui.add_space(6.0);
    }
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let police = FontId::proportional(13.0);
    let plus_long = g
        .etiquettes
        .iter()
        .map(|e| p.layout_no_wrap(e.clone(), police.clone(), erp::TEXTE).size().x)
        .fold(0.0, f32::max);
    let l_etiquette = (plus_long + 12.0).min(largeur * 0.4);
    let haut = g.series.iter().flat_map(|s| s.valeurs.iter().flatten().copied()).fold(0.0_f64, f64::max).max(1e-9);
    let l_valeur = g
        .series
        .iter()
        .flat_map(|s| s.valeurs.iter().flatten())
        .map(|v| p.layout_no_wrap(compact(*v, &g.unite), police_de_valeur(), erp::TITRE).size().x)
        .fold(0.0, f32::max)
        + 8.0;
    let epaisseur = if k == 1 { 14.0 } else { 10.0 };
    let h_rangee = epaisseur * k as f32 + INTERVALLE * (k - 1) as f32 + 12.0;
    let n = g.etiquettes.len();
    let (rect, rep) = ui.allocate_exact_size(vec2(largeur, h_rangee * n as f32), Sense::hover());
    let x0 = rect.left() + l_etiquette;
    let plage = (rect.right() - l_valeur - x0).max(10.0);
    for (i, e) in g.etiquettes.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left(), rect.top() + h_rangee * i as f32), vec2(largeur, h_rangee));
        let survolee = rep.hover_pos().filter(|pt| r.contains(*pt));
        if survolee.is_some() {
            p.rect_filled(r, 6.0, erp::SURVOL);
        }
        let mut job = egui::text::LayoutJob::simple_singleline(e.clone(), police.clone(), erp::TEXTE);
        job.wrap = egui::text::TextWrapping::truncate_at_width(l_etiquette - 12.0);
        let nom = p.layout_job(job);
        p.galley(pos2(r.left() + 4.0, r.center().y - nom.size().y / 2.0), nom, erp::TEXTE);
        for s in 0..k {
            let Some(v) = g.series[s].valeurs[i] else { continue };
            let yy = r.top() + 6.0 + s as f32 * (epaisseur + INTERVALLE);
            let l = (v.max(0.0) / haut) as f32 * plage;
            if l >= 0.5 {
                let rayon = 4.0_f32.min(epaisseur / 2.0).min(l) as u8;
                let barre = Rect::from_min_size(pos2(x0, yy), vec2(l, epaisseur));
                p.rect_filled(barre, CornerRadius { nw: 0, sw: 0, ne: rayon, se: rayon }, g.couleur(s));
            }
            let texte = p.layout_no_wrap(compact(v, &g.unite), police_de_valeur(), erp::TITRE);
            p.galley(pos2(x0 + l + 6.0, yy + epaisseur / 2.0 - texte.size().y / 2.0), texte, erp::TITRE);
        }
        if let Some(pt) = survolee
            && k > 1
        {
            let lignes: Vec<(Color32, String, String)> =
                (0..k).filter_map(|s| g.series[s].valeurs[i].map(|v| (g.couleur(s), g.dite(v), g.nom(s)))).collect();
            bulle(ui, pt, e, &lignes, cle);
        }
    }
}

/// Un anneau : ses parts dans le sens des aiguilles d'une montre depuis le haut, deux points de blanc entre elles ; au
/// milieu, le total ; à côté (ou dessous, dans une carte étroite), la légende — chaque part, sa valeur, sa part du total.
/// La part survolée s'avance un peu, et sa bulle la dit.
fn anneau(ui: &mut egui::Ui, g: &Graphique, cle: &str) {
    let valeurs: Vec<f64> = g.series[0].valeurs.iter().map(|v| v.unwrap_or(0.0)).collect();
    let total: f64 = valeurs.iter().sum();
    let largeur = ui.available_width();
    let cote = 150.0_f32.min(largeur);
    let cote_a_cote = largeur >= 340.0;
    let p = ui.painter().clone();
    let police = FontId::proportional(12.5);
    let legendes: Vec<(Arc<egui::Galley>, Arc<egui::Galley>)> = g
        .etiquettes
        .iter()
        .zip(&valeurs)
        .map(|(e, v)| {
            let chiffres = format!("{} · {}", compact(*v, &g.unite), pourcent(v / total));
            (
                p.layout_no_wrap(e.clone(), police.clone(), erp::TEXTE),
                p.layout_no_wrap(chiffres, police_de_valeur(), erp::TITRE),
            )
        })
        .collect();
    let h_legende = 22.0 * legendes.len() as f32;
    let hauteur = if cote_a_cote { cote.max(h_legende) } else { cote + 12.0 + h_legende };
    let (rect, rep) = ui.allocate_exact_size(vec2(largeur, hauteur), Sense::hover());
    let centre = if cote_a_cote {
        pos2(rect.left() + cote / 2.0, rect.top() + hauteur / 2.0)
    } else {
        pos2(rect.center().x, rect.top() + cote / 2.0)
    };
    let (exterieur, interieur) = (cote / 2.0 - 5.0, cote / 2.0 - 27.0);
    let survol = rep.hover_pos().and_then(|pt| {
        let d = pt - centre;
        let r = d.length();
        if r < interieur - 4.0 || r > exterieur + 6.0 {
            return None;
        }
        let a = d.x.atan2(-d.y).rem_euclid(TAU);
        let mut debut = 0.0;
        valeurs.iter().position(|v| {
            let fin = debut + (*v / total) as f32 * TAU;
            let dedans = a >= debut && a < fin;
            debut = fin;
            dedans
        })
    });
    let ecart = INTERVALLE / ((exterieur + interieur) / 2.0);
    let mut debut = 0.0;
    for (k, v) in valeurs.iter().enumerate() {
        let fin = debut + (*v / total) as f32 * TAU;
        let avance = if survol == Some(k) { 3.0 } else { 0.0 };
        if fin - debut > ecart {
            secteur(&p, centre, interieur, exterieur + avance, debut + ecart / 2.0, fin - ecart / 2.0, g.couleur(k));
        }
        debut = fin;
    }
    let somme =
        p.layout_no_wrap(compact(total, &g.unite), FontId::new(18.0, FontFamily::Name(SEMI.into())), erp::TITRE);
    let dit = p.layout_no_wrap("au total".into(), FontId::proportional(11.0), erp::TEXTE_2);
    let (l_somme, h_somme) = (somme.size().x, somme.size().y);
    let h = h_somme + dit.size().y;
    p.galley(pos2(centre.x - l_somme / 2.0, centre.y - h / 2.0), somme, erp::TITRE);
    p.galley(pos2(centre.x - dit.size().x / 2.0, centre.y - h / 2.0 + h_somme), dit, erp::TEXTE_2);
    let (gauche, haut) = if cote_a_cote {
        (rect.left() + cote + 22.0, rect.top() + (hauteur - h_legende) / 2.0)
    } else {
        (rect.left(), rect.top() + cote + 12.0)
    };
    for (k, (nom, chiffres)) in legendes.into_iter().enumerate() {
        let yy = haut + 22.0 * k as f32 + 11.0;
        p.rect_filled(Rect::from_center_size(pos2(gauche + 5.0, yy), vec2(10.0, 10.0)), 2.0, g.couleur(k));
        let place = (rect.right() - gauche - 18.0 - chiffres.size().x - 10.0).max(20.0);
        let mut job = egui::text::LayoutJob::simple_singleline(g.etiquettes[k].clone(), police.clone(), erp::TEXTE);
        job.wrap = egui::text::TextWrapping::truncate_at_width(place);
        let nom = if nom.size().x > place { p.layout_job(job) } else { nom };
        p.galley(pos2(gauche + 18.0, yy - nom.size().y / 2.0), nom, erp::TEXTE);
        p.galley(pos2(rect.right() - chiffres.size().x, yy - chiffres.size().y / 2.0), chiffres, erp::TITRE);
    }
    if let (Some(k), Some(pt)) = (survol, rep.hover_pos()) {
        let ligne = (g.couleur(k), g.dite(valeurs[k]), pourcent(valeurs[k] / total));
        bulle(ui, pt, &g.etiquettes[k], &[ligne], cle);
    }
}

/// Une part d'un anneau, de l'angle `debut` à `fin` (depuis le haut, dans le sens des aiguilles d'une montre), entre
/// les rayons `interieur` et `exterieur` : un maillage, assez fin pour que son bord reste rond.
fn secteur(p: &egui::Painter, c: Pos2, interieur: f32, exterieur: f32, debut: f32, fin: f32, couleur: Color32) {
    let pas = ((fin - debut) * exterieur / 3.0).ceil().max(2.0) as u32;
    let mut maille = Mesh::default();
    for t in 0..=pas {
        let a = debut + (fin - debut) * t as f32 / pas as f32;
        let (s, co) = a.sin_cos();
        let direction = vec2(s, -co);
        maille.colored_vertex(c + direction * exterieur, couleur);
        maille.colored_vertex(c + direction * interieur, couleur);
    }
    for t in 0..pas {
        let i = t * 2;
        maille.add_triangle(i, i + 1, i + 2);
        maille.add_triangle(i + 1, i + 3, i + 2);
    }
    p.add(Shape::mesh(maille));
}

/// La légende de plusieurs séries, sur une rangée qui passe à la ligne : chaque série, sa clé (un trait pour une
/// ligne, un carré pour une colonne ou une aire) et son nom, dans l'encre du texte.
fn legende(ui: &mut egui::Ui, g: &Graphique) {
    let trait_de_ligne = g.forme == Forme::Lignes;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 6.0);
        for k in 0..g.series.len() {
            let nom = ui.painter().layout_no_wrap(g.nom(k), FontId::proportional(12.5), erp::TEXTE);
            let (r, _) = ui.allocate_exact_size(vec2(18.0 + nom.size().x, nom.size().y.max(12.0)), Sense::hover());
            let p = ui.painter();
            let c = pos2(r.left() + 6.0, r.center().y);
            if trait_de_ligne {
                p.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], Stroke::new(2.0, g.couleur(k)));
            } else {
                p.rect_filled(Rect::from_center_size(c, vec2(10.0, 10.0)), 2.0, g.couleur(k));
            }
            p.galley(pos2(r.left() + 18.0, r.center().y - nom.size().y / 2.0), nom, erp::TEXTE);
        }
    });
}

/// La bulle d'un survol, au-dessus de tout, près du pointeur sans sortir de la fenêtre : son titre (l'étiquette), puis
/// une ligne par série — sa clé (un trait de sa couleur), sa valeur en demi-gras d'abord, son nom en gris.
fn bulle(ui: &egui::Ui, pres: Pos2, titre: &str, lignes: &[(Color32, String, String)], cle: &str) {
    let couche = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique(("bulle-de-graphique", cle)));
    let p = ui.ctx().layer_painter(couche);
    let g_titre = p.layout_no_wrap(titre.to_string(), FontId::proportional(12.0), erp::TEXTE_2);
    let rangs: Vec<(Color32, Arc<egui::Galley>, Arc<egui::Galley>)> = lignes
        .iter()
        .map(|(c, v, n)| {
            let valeur = p.layout_no_wrap(v.clone(), FontId::new(13.0, FontFamily::Name(SEMI.into())), erp::TITRE);
            (*c, valeur, p.layout_no_wrap(n.clone(), FontId::proportional(12.0), erp::TEXTE_2))
        })
        .collect();
    let l_rangs = rangs.iter().map(|(_, v, n)| 22.0 + v.size().x + 8.0 + n.size().x).fold(0.0, f32::max);
    let largeur = g_titre.size().x.max(l_rangs) + 24.0;
    let h_rangs: f32 = rangs.iter().map(|(_, v, n)| v.size().y.max(n.size().y) + 3.0).sum();
    let hauteur = 10.0 + g_titre.size().y + 5.0 + h_rangs + 8.0;
    let ecran = ui.ctx().content_rect();
    let mut coin = pres + vec2(16.0, 16.0);
    if coin.x + largeur > ecran.right() - 4.0 {
        coin.x = pres.x - 16.0 - largeur;
    }
    if coin.y + hauteur > ecran.bottom() - 4.0 {
        coin.y = pres.y - 16.0 - hauteur;
    }
    let r = Rect::from_min_size(coin, vec2(largeur, hauteur));
    p.add(theme::ombre_marine(r, 8, 4, 14, 46));
    p.rect(r, 8.0, Color32::WHITE, Stroke::new(1.0, erp::FILET_DE_CARTE), egui::StrokeKind::Inside);
    let mut yy = r.top() + 10.0;
    p.galley(pos2(r.left() + 12.0, yy), g_titre.clone(), erp::TEXTE_2);
    yy += g_titre.size().y + 5.0;
    for (couleur, valeur, nom) in rangs {
        let h = valeur.size().y.max(nom.size().y);
        let cy = yy + h / 2.0;
        p.line_segment([pos2(r.left() + 12.0, cy), pos2(r.left() + 26.0, cy)], Stroke::new(2.0, couleur));
        let x = r.left() + 34.0;
        let l_valeur = valeur.size().x;
        p.galley(pos2(x, cy - valeur.size().y / 2.0), valeur, erp::TITRE);
        p.galley(pos2(x + l_valeur + 8.0, cy - nom.size().y / 2.0), nom, erp::TEXTE_2);
        yy += h + 3.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lu(json: &str) -> Result<Graphique, String> {
        Graphique::de(&serde_json::from_str(json).unwrap())
    }

    /// Les formes et leurs synonymes ; les séries en objets ou en listes, des valeurs en texte, une qui manque ; des
    /// étiquettes qui manquent ; la série soulignée ; ce qui ne se dessine pas dit pourquoi.
    #[test]
    fn un_graphique_se_lit_d_une_carte() {
        let g = lu(r#"{"forme":"Courbes","etiquettes":["janv.","févr.","mars"],"series":[{"nom":"2026","valeurs":[12,"13,5",null]},{"nom":"2025","valeurs":[10,11,12,99]}],"unite":"k$","souligne":"2026"}"#).unwrap();
        assert_eq!(g.forme, Forme::Lignes);
        assert_eq!(g.series[0].valeurs, [Some(12.0), Some(13.5), None]);
        assert_eq!(g.series[1].valeurs.len(), 3, "une valeur de trop s'en va");
        assert_eq!((g.souligne, g.couleur(0), g.couleur(1)), (Some(0), SERIES[0], CONTEXTE));
        let g = lu(r#"{"series":[[1,2,3]]}"#).unwrap();
        assert_eq!(
            (g.forme, g.etiquettes.clone(), g.nom(0)),
            (Forme::Colonnes, vec!["1".into(), "2".into(), "3".into()], "Valeur".into())
        );
        let g = lu(r#"{"forme":"horizontales","etiquettes":["a","b"],"valeurs":["1 250","x"]}"#).unwrap();
        assert_eq!(g.series[0].valeurs, [Some(1250.0), None]);
        assert!(lu(r#"{"forme":"radar","valeurs":[1]}"#).unwrap_err().contains("radar"));
        assert!(lu(r#"{"forme":"barres","valeurs":[null]}"#).is_err());
        assert!(lu(r#"{"forme":"anneau","etiquettes":["a"],"valeurs":[0]}"#).is_err());
    }

    /// Un anneau : six parts au plus, la sixième réunit les autres (en gris) ; les parts nulles s'en vont ; sa vue en
    /// tableau dit chaque part et sa part du total.
    #[test]
    fn un_anneau_plie_ses_parts_et_se_lit_en_tableau() {
        let g = lu(r#"{"forme":"anneau","etiquettes":["a","b","c","d","e","f","g","h"],"valeurs":[40,20,10,10,5,0,10,5],"unite":"$"}"#).unwrap();
        assert_eq!(g.etiquettes, ["a", "b", "c", "d", "e", "Autres"]);
        assert_eq!(g.series[0].valeurs[5], Some(15.0));
        assert_eq!((g.couleur(4), g.couleur(5)), (SERIES[4], CONTEXTE));
        let t: Value = serde_json::from_str(&g.en_tableau()).unwrap();
        assert_eq!(t["lignes"][0], json!(["a", "40\u{a0}$", "40\u{a0}%"]));
        assert_eq!(t["pied"], json!(["Total", "100\u{a0}$", "100\u{a0}%"]));
    }

    /// La vue en tableau d'un graphique : une colonne par série, ses noms ; une valeur qui manque, une case vide.
    #[test]
    fn un_graphique_se_lit_en_tableau() {
        let g = lu(r#"{"etiquettes":["T1","T2"],"series":[{"nom":"Ventes","valeurs":[1250.5,null]},{"valeurs":[3,4]}],"unite":"$"}"#).unwrap();
        let t: Value = serde_json::from_str(&g.en_tableau()).unwrap();
        assert_eq!(t["colonnes"], json!(["", "Ventes", "Série 2"]));
        assert_eq!(t["lignes"][0], json!(["T1", "1\u{202f}250,5\u{a0}$", "3\u{a0}$"]));
        assert_eq!(t["lignes"][1], json!(["T2", "", "4\u{a0}$"]));
    }

    /// Les nombres à la française, compacts dès dix mille, avec leur unité.
    #[test]
    fn les_nombres_se_disent_a_la_francaise() {
        assert_eq!(nombre_fr(1250.5), "1\u{202f}250,5");
        assert_eq!(nombre_fr(-0.25), "-0,25");
        assert_eq!(nombre_fr(3.0), "3");
        assert_eq!(compact(940.0, "$"), "940\u{a0}$");
        assert_eq!(compact(12_540.0, "$"), "12,5\u{a0}k$");
        assert_eq!(compact(1_250_000.0, "$"), "1,3\u{a0}M$");
        assert_eq!(compact(45.0, "%"), "45\u{a0}%");
        assert_eq!(compact(2_500.0, ""), "2\u{202f}500");
        assert_eq!(pourcent(0.004), "moins de 1\u{a0}%");
    }

    /// Les graduations : des nombres ronds, zéro compris, qui couvrent l'étendue.
    #[test]
    fn les_graduations_sont_rondes() {
        assert_eq!(graduer(0.0, 95.0), [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(graduer(0.0, 4.0), [0.0, 1.0, 2.0, 3.0, 4.0]);
        let negatives = graduer(-30.0, 80.0);
        assert!(
            negatives.contains(&0.0) && negatives[0] <= -30.0 && *negatives.last().unwrap() >= 80.0,
            "{negatives:?}"
        );
        let rien = graduer(0.0, 0.0);
        assert!(rien.len() == 6 && rien[0] == 0.0 && (rien[5] - 1.0).abs() < 1e-9, "{rien:?}");
    }
}
