//! Le calcul des sommaires de l'onglet « Comptabilité » : par mois, par fournisseur, par chantier (la marge avant
//! taxes), en centimes. Son dessin : `natif\compta\sommaires.rs`.
//!
//! Ce qui compte : les factures que retiennent les filtres d'en tête ([`Filtres::retient`]), puis les règles du contrat
//! — une dépense ([`Facture::depense`] : la facture d'un fournisseur, brouillon compris, ni annulée ni pro forma), un
//! revenu ([`Facture::revenu`] : une facture émise à un client), la note de crédit qui se retranche ([`Somme`]), ce qui
//! reste à payer ou à recevoir ([`Facture::reste`]). Une facture qui n'est ni l'un ni l'autre (une pro forma, une
//! annulée, le brouillon d'une facture à un client) n'entre dans aucun sommaire. Rien n'est quadratique : un passage
//! sur les factures, un tri des rangées (vingt mille factures, quelques centaines de rangées).
//!
//! La marge se calcule avant taxes, comme dans EG : les revenus avant taxes, moins les dépenses avant taxes (la TPS et
//! la TVQ se remettent ou se récupèrent ; elles n'en font pas partie).

use std::collections::HashMap;

use super::{Cents, Chantier, Cote, Filtres, Registre, Somme};

/// Deux sommes l'une sur l'autre (la rangée TOTAL).
fn plus(a: Somme, b: &Somme) -> Somme {
    Somme {
        n: a.n + b.n,
        sous_total: a.sous_total + b.sous_total,
        tps: a.tps + b.tps,
        tvq: a.tvq + b.tvq,
        total: a.total + b.total,
        sans_total: a.sans_total + b.sans_total,
    }
}

/// `part` sur `de`, en dixièmes de pour cent, la demie loin de zéro (123 : 12,3 %) ; `None` quand `de` n'est pas plus
/// grand que zéro (une part de rien ne se dit pas). En entiers, jusqu'au bout.
pub fn dixiemes_de_pour_cent(part: Cents, de: Cents) -> Option<i64> {
    if de <= 0 {
        return None;
    }
    let (p, d) = (i128::from(part) * 1000, i128::from(de));
    let q = (p.abs() * 2 + d) / (2 * d);
    i64::try_from(if p < 0 { -q } else { q }).ok()
}

/// 123 → « 12,3 % », −45 → « −4,5 % » (le signe moins typographique, comme l'argent ; une espace insécable avant le
/// signe de pour cent).
pub fn pour_cent(dixiemes: i64) -> String {
    let signe = if dixiemes < 0 { "\u{2212}" } else { "" };
    let n = dixiemes.unsigned_abs();
    // Les milliers séparés, comme l'argent (« −2 193,3 % » : la QA5).
    format!("{signe}{},{}\u{a0}%", super::par_milliers(n / 10), n % 10)
}

// ---- Par mois -------------------------------------------------------------------------------------------------------

/// Une rangée du sommaire par mois : le mois, « AAAA-MM » (`None` : les factures sans date, que seule « Toute la
/// période » retient), ses dépenses et ses revenus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mois {
    pub mois: Option<String>,
    pub depenses: Somme,
    pub revenus: Somme,
}

impl Mois {
    /// La marge avant taxes : les revenus avant taxes, moins les dépenses avant taxes.
    pub fn marge(&self) -> Cents {
        self.revenus.sous_total - self.depenses.sous_total
    }
}

/// Le sommaire par mois : le plus récent en haut, les factures sans date au pied ; et la rangée TOTAL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParMois {
    pub rangees: Vec<Mois>,
    pub total: Mois,
}

/// Les dépenses et les revenus de chaque mois, d'après les factures que retiennent les filtres (`aujourdhui` :
/// « AAAA-MM-JJ », le repère des douze derniers mois).
pub fn par_mois(r: &Registre, filtres: &Filtres, aujourdhui: &str) -> ParMois {
    let mut par: HashMap<Option<&str>, Mois> = HashMap::new();
    for f in r.factures.iter().filter(|f| filtres.retient(f, aujourdhui)) {
        let (depense, revenu) = (f.depense(), f.revenu());
        if !depense && !revenu {
            continue;
        }
        let m = par.entry(f.mois()).or_insert_with(|| Mois { mois: f.mois().map(str::to_string), ..Mois::default() });
        if depense {
            m.depenses.ajouter(f);
        } else {
            m.revenus.ajouter(f);
        }
    }
    let mut rangees: Vec<Mois> = par.into_values().collect();
    // Le plus récent en haut (« AAAA-MM » se range comme un texte) ; sans date, au pied.
    rangees.sort_by(|a, b| match (&a.mois, &b.mois) {
        (Some(x), Some(y)) => y.cmp(x),
        (x, y) => x.is_none().cmp(&y.is_none()),
    });
    let total = rangees.iter().fold(Mois::default(), |t, m| Mois {
        mois: None,
        depenses: plus(t.depenses, &m.depenses),
        revenus: plus(t.revenus, &m.revenus),
    });
    ParMois { rangees, total }
}

// ---- Par fournisseur ------------------------------------------------------------------------------------------------

/// Une rangée du sommaire par fournisseur : son nom tel que l'ERP l'écrit (vide : des factures sans fournisseur), ses
/// dépenses, et ce qui reste à lui payer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fournisseur {
    pub nom: String,
    pub depenses: Somme,
    pub reste: Cents,
}

/// Le sommaire par fournisseur : du plus gros total au plus petit ; et la rangée TOTAL (son nom vide).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParFournisseur {
    pub rangees: Vec<Fournisseur>,
    pub total: Fournisseur,
}

impl ParFournisseur {
    /// La part d'un fournisseur dans les dépenses (leurs totaux, taxes comprises), en dixièmes de pour cent ; `None`
    /// quand les dépenses ne font pas un total plus grand que zéro, ou que le sien est négatif (des notes de crédit).
    pub fn part(&self, f: &Fournisseur) -> Option<i64> {
        if f.depenses.total < 0 {
            return None;
        }
        dixiemes_de_pour_cent(f.depenses.total, self.total.depenses.total)
    }
}

/// Les dépenses de chaque fournisseur (les factures d'un fournisseur qui sont des dépenses, et que retiennent les
/// filtres). Un fournisseur se reconnaît à son nom sans la casse, les accents ni la ponctuation
/// (`eve::parole::normaliser`) : c'est ainsi que la liste des fournisseurs, qu'ouvre un clic sur sa rangée, filtre le
/// tiers (`compta\listes.rs`) — « Béton Fictif » et « BÉTON FICTIF » sont un seul fournisseur, nommé comme sa
/// première facture.
pub fn par_fournisseur(r: &Registre, filtres: &Filtres, aujourdhui: &str) -> ParFournisseur {
    let mut par: HashMap<String, Fournisseur> = HashMap::new();
    for f in r.factures.iter().filter(|f| f.depense() && filtres.retient(f, aujourdhui)) {
        let nom = f.tiers.trim();
        let cle = eve::parole::normaliser(nom);
        let ligne = par.entry(cle).or_insert_with(|| Fournisseur { nom: nom.to_string(), ..Fournisseur::default() });
        ligne.depenses.ajouter(f);
        ligne.reste += f.reste();
    }
    let mut rangees: Vec<Fournisseur> = par.into_values().collect();
    // Du plus gros au plus petit ; à total égal, par leur nom (sans la casse), puis tel qu'écrit : un ordre stable.
    rangees.sort_by(|a, b| {
        b.depenses
            .total
            .cmp(&a.depenses.total)
            .then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()))
            .then_with(|| a.nom.cmp(&b.nom))
    });
    let total = rangees.iter().fold(Fournisseur::default(), |t, l| Fournisseur {
        nom: String::new(),
        depenses: plus(t.depenses, &l.depenses),
        reste: t.reste + l.reste,
    });
    ParFournisseur { rangees, total }
}

// ---- Par chantier ---------------------------------------------------------------------------------------------------

/// Une rangée du sommaire par chantier : le chantier (un projet de l'ERP, ou `Chantier::Aucun` pour les factures sans
/// chantier ; `Chantier::Tous` pour la rangée TOTAL) et son nom, ses dépenses et ses revenus, ce qui reste à recevoir
/// de ses clients, et les factures comptées dont la base n'a pas de sous-total (la marge ne les voit pas).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChantierLigne {
    pub chantier: Chantier,
    pub nom: String,
    pub depenses: Somme,
    pub revenus: Somme,
    pub a_recevoir: Cents,
    pub sans_sous_total: u32,
}

impl ChantierLigne {
    /// La marge avant taxes : les revenus avant taxes, moins les dépenses avant taxes.
    pub fn marge(&self) -> Cents {
        self.revenus.sous_total - self.depenses.sous_total
    }

    /// Le taux de marge : la marge sur les revenus avant taxes, en dixièmes de pour cent (négatif : une perte) ;
    /// `None` sans revenu avant taxes plus grand que zéro.
    pub fn taux_de_marge(&self) -> Option<i64> {
        dixiemes_de_pour_cent(self.marge(), self.revenus.sous_total)
    }

    /// Les factures comptées : les dépenses et les revenus.
    pub fn n(&self) -> u32 {
        self.depenses.n + self.revenus.n
    }
}

/// Le sommaire par chantier : les chantiers par leur nom (sans la casse), « Sans chantier » au pied ; et la rangée
/// TOTAL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParChantier {
    pub rangees: Vec<ChantierLigne>,
    pub total: ChantierLigne,
}

/// Le nom des factures sans chantier.
pub const SANS_CHANTIER: &str = "Sans chantier";

/// Les dépenses, les revenus et la marge de chaque chantier, d'après les factures que retiennent les filtres.
pub fn par_chantier(r: &Registre, filtres: &Filtres, aujourdhui: &str) -> ParChantier {
    let mut par: HashMap<Option<i64>, ChantierLigne> = HashMap::new();
    for f in r.factures.iter().filter(|f| filtres.retient(f, aujourdhui)) {
        let (depense, revenu) = (f.depense(), f.revenu());
        if !depense && !revenu {
            continue;
        }
        let ligne = par.entry(f.projet_id).or_insert_with(|| ChantierLigne {
            chantier: f.projet_id.map_or(Chantier::Aucun, Chantier::Projet),
            ..ChantierLigne::default()
        });
        // Le nom : le premier que donne une de ses factures ; sinon, comme le choix du chantier d'en tête.
        if ligne.nom.is_empty() {
            ligne.nom = match (f.projet_id, f.projet.as_deref().map(str::trim)) {
                (None, _) => SANS_CHANTIER.to_string(),
                (Some(_), Some(nom)) if !nom.is_empty() => nom.to_string(),
                (Some(_), _) => String::new(),
            };
        }
        if f.sous_total.is_none() {
            ligne.sans_sous_total += 1;
        }
        if depense {
            ligne.depenses.ajouter(f);
        } else {
            ligne.revenus.ajouter(f);
            if f.cote == Cote::Client {
                ligne.a_recevoir += f.reste();
            }
        }
    }
    let mut rangees: Vec<ChantierLigne> = par.into_values().collect();
    for l in &mut rangees {
        if let (Chantier::Projet(id), true) = (l.chantier, l.nom.is_empty()) {
            l.nom = format!("Projet {id}");
        }
    }
    let id = |c: Chantier| if let Chantier::Projet(id) = c { id } else { i64::MAX };
    // Par leur nom sans accents ni casse (« École » parmi les « E », pas après « Projet 11 » : la QA5 du lot 13.2),
    // « Sans chantier » en dernier.
    rangees.sort_by_cached_key(|l| (l.chantier == Chantier::Aucun, eve::parole::normaliser(&l.nom), id(l.chantier)));
    let total = rangees.iter().fold(ChantierLigne { chantier: Chantier::Tous, ..ChantierLigne::default() }, |t, l| {
        ChantierLigne {
            chantier: Chantier::Tous,
            nom: String::new(),
            depenses: plus(t.depenses, &l.depenses),
            revenus: plus(t.revenus, &l.revenus),
            a_recevoir: t.a_recevoir + l.a_recevoir,
            sans_sous_total: t.sans_sous_total + l.sans_sous_total,
        }
    });
    ParChantier { rangees, total }
}

#[cfg(test)]
mod essais {
    use super::super::{Facture, Periode};
    use super::*;

    /// Une facture du petit registre des essais : ses montants en centimes, écrits à la main.
    #[allow(clippy::too_many_arguments)]
    fn f(
        id: i64,
        cote: Cote,
        type_document: &str,
        statut: &str,
        date: Option<&str>,
        tiers: &str,
        projet: Option<(i64, &str)>,
        montants: Option<(Cents, Cents, Cents)>,
        solde: Cents,
    ) -> Facture {
        let (sous_total, tps, tvq) = match montants {
            Some((s, t, q)) => (Some(s), Some(t), Some(q)),
            None => (None, None, None),
        };
        Facture {
            id,
            numero: format!("F-{id}"),
            cote,
            type_document: type_document.into(),
            statut: statut.into(),
            date: date.map(str::to_string),
            tiers: tiers.into(),
            projet_id: projet.map(|p| p.0),
            projet: projet.map(|p| p.1.to_string()),
            sous_total,
            tps,
            tvq,
            total: montants.map(|(s, t, q)| s + t + q),
            solde: Some(solde),
            ..Facture::default()
        }
    }

    /// Le petit registre des essais, écrit à la main : deux chantiers (7 « Entrepôt », 9 « Condos »), deux
    /// fournisseurs, deux clients ; une note de crédit, un brouillon de chaque côté, une annulée, une pro forma, une
    /// sans date, une sans chantier, une sans total.
    fn registre() -> Registre {
        use Cote::{Client, Fournisseur};
        let e = Some((7, "Entrepôt"));
        let c = Some((9, "Condos"));
        let factures = vec![
            // Octobre 2026.
            f(
                1,
                Fournisseur,
                "FACTURE",
                "PAYEE",
                Some("2026-10-05"),
                "Béton Fictif",
                e,
                Some((100_000, 5_000, 9_975)),
                0,
            ),
            f(
                2,
                Fournisseur,
                "FACTURE",
                "ENVOYEE",
                Some("2026-10-20"),
                "Bois Modèle",
                c,
                Some((50_000, 2_500, 4_988)),
                57_488,
            ),
            f(
                3,
                Client,
                "FACTURE",
                "ENVOYEE",
                Some("2026-10-25"),
                "Groupe Fictif",
                e,
                Some((300_000, 15_000, 29_925)),
                344_925,
            ),
            // Une note de crédit au client de l'entrepôt, en octobre : elle se retranche.
            f(
                4,
                Client,
                "AVOIR",
                "ENVOYEE",
                Some("2026-10-28"),
                "Groupe Fictif",
                e,
                Some((20_000, 1_000, 1_995)),
                22_995,
            ),
            // Septembre 2026.
            f(
                5,
                Fournisseur,
                "FACTURE",
                "BROUILLON",
                Some("2026-09-10"),
                "Béton Fictif",
                e,
                Some((40_000, 2_000, 3_990)),
                45_990,
            ),
            f(
                6,
                Client,
                "FACTURE",
                "PARTIELLEMENT_PAYEE",
                Some("2026-09-25"),
                "Habitations Exemple",
                c,
                Some((200_000, 10_000, 19_950)),
                100_000,
            ),
            // Le brouillon d'une facture à un client : pas encore émise, il ne compte pas.
            f(
                7,
                Client,
                "FACTURE",
                "BROUILLON",
                Some("2026-09-26"),
                "Habitations Exemple",
                c,
                Some((99_900, 0, 0)),
                99_900,
            ),
            // Une annulée, une pro forma : elles ne comptent nulle part.
            f(
                8,
                Fournisseur,
                "FACTURE",
                "ANNULEE",
                Some("2026-09-12"),
                "Bois Modèle",
                c,
                Some((70_000, 3_500, 6_983)),
                80_483,
            ),
            f(9, Client, "PROFORMA", "ENVOYEE", Some("2026-09-14"), "Groupe Fictif", e, Some((500_000, 0, 0)), 500_000),
            // Sans chantier, en octobre, chez un troisième fournisseur.
            f(
                10,
                Fournisseur,
                "FACTURE",
                "ENVOYEE",
                Some("2026-10-02"),
                "Bureau Fictif",
                None,
                Some((10_000, 500, 998)),
                11_498,
            ),
            // Sans date : seule « Toute la période » la retient.
            f(11, Fournisseur, "FACTURE", "ENVOYEE", None, "Bois Modèle", c, Some((5_000, 250, 499)), 5_749),
            // Sans total (ni sous-total) : elle compte pour une facture, pour zéro dollar.
            f(12, Fournisseur, "FACTURE", "BROUILLON", Some("2026-10-30"), "Bureau Fictif", None, None, 0),
            // Août 2025 : une autre année.
            f(
                13,
                Fournisseur,
                "FACTURE",
                "PAYEE",
                Some("2025-08-15"),
                "Béton Fictif",
                e,
                Some((30_000, 1_500, 2_993)),
                0,
            ),
        ];
        Registre { au_serveur: factures.len() as u64, factures, ..Registre::default() }
    }

    const AUJOURDHUI: &str = "2026-11-02";

    fn tout() -> Filtres {
        Filtres::default()
    }

    #[test]
    fn par_mois_les_totaux_au_cent_et_l_ordre() {
        let s = par_mois(&registre(), &tout(), AUJOURDHUI);
        let mois: Vec<Option<&str>> = s.rangees.iter().map(|m| m.mois.as_deref()).collect();
        assert_eq!(mois, [Some("2026-10"), Some("2026-09"), Some("2025-08"), None], "le plus récent en haut");
        // Octobre : les dépenses 1, 2, 10 et 12 (sans total) ; les revenus 3, moins la note de crédit 4.
        let octobre = &s.rangees[0];
        let depenses = Somme { n: 4, sous_total: 160_000, tps: 8_000, tvq: 15_961, total: 183_961, sans_total: 1 };
        assert_eq!(octobre.depenses, depenses);
        let revenus = Somme { n: 2, sous_total: 280_000, tps: 14_000, tvq: 27_930, total: 321_930, sans_total: 0 };
        assert_eq!(octobre.revenus, revenus);
        assert_eq!(octobre.marge(), 120_000);
        // Septembre : le brouillon du fournisseur compte ; celui du client, l'annulée et la pro forma, non.
        let septembre = &s.rangees[1];
        assert_eq!((septembre.depenses.n, septembre.depenses.total), (1, 45_990));
        assert_eq!((septembre.revenus.n, septembre.revenus.sous_total, septembre.revenus.total), (1, 200_000, 229_950));
        assert_eq!(septembre.marge(), 160_000);
        // Sans date : au pied.
        assert_eq!((s.rangees[3].depenses.n, s.rangees[3].depenses.total), (1, 5_749));
        // Le TOTAL : 7 dépenses, 3 revenus.
        assert_eq!(
            s.total.depenses,
            Somme { n: 7, sous_total: 235_000, tps: 11_750, tvq: 23_443, total: 270_193, sans_total: 1 }
        );
        assert_eq!(
            s.total.revenus,
            Somme { n: 3, sous_total: 480_000, tps: 24_000, tvq: 47_880, total: 551_880, sans_total: 0 }
        );
        assert_eq!(s.total.marge(), 245_000);
    }

    #[test]
    fn par_mois_suit_les_filtres() {
        let r = registre();
        // Les douze derniers mois (de décembre 2025 à novembre 2026) : ni août 2025 ni la facture sans date.
        let douze = Filtres { periode: Periode::DouzeMois, ..tout() };
        let s = par_mois(&r, &douze, AUJOURDHUI);
        let mois: Vec<Option<&str>> = s.rangees.iter().map(|m| m.mois.as_deref()).collect();
        assert_eq!(mois, [Some("2026-10"), Some("2026-09")]);
        assert_eq!((s.total.depenses.n, s.total.depenses.total), (5, 229_951));
        // Le chantier 9 seulement : la facture 2 en octobre, la 6 en septembre (sans date : la 11).
        let condos = Filtres { chantier: Chantier::Projet(9), ..tout() };
        let s = par_mois(&r, &condos, AUJOURDHUI);
        assert_eq!(s.rangees.len(), 3);
        assert_eq!((s.total.depenses.n, s.total.depenses.total, s.total.revenus.total), (2, 63_237, 229_950));
        // Sans chantier : les factures 10 et 12.
        let sans = Filtres { chantier: Chantier::Aucun, ..tout() };
        assert_eq!(
            par_mois(&r, &sans, AUJOURDHUI).total.depenses,
            Somme { n: 2, sous_total: 10_000, tps: 500, tvq: 998, total: 11_498, sans_total: 1 }
        );
    }

    #[test]
    fn par_fournisseur_du_plus_gros_au_plus_petit() {
        let s = par_fournisseur(&registre(), &tout(), AUJOURDHUI);
        let noms: Vec<&str> = s.rangees.iter().map(|l| l.nom.as_str()).collect();
        assert_eq!(noms, ["Béton Fictif", "Bois Modèle", "Bureau Fictif"]);
        // Béton Fictif : 1, 5 (un brouillon) et 13 ; rien ne reste à lui payer (deux sont payées, un brouillon ne doit
        // rien encore).
        let beton = &s.rangees[0];
        assert_eq!(
            beton.depenses,
            Somme { n: 3, sous_total: 170_000, tps: 8_500, tvq: 16_958, total: 195_458, sans_total: 0 }
        );
        assert_eq!(beton.reste, 0, "la 1 et la 13 sont payées, la 5 est un brouillon");
        // Bois Modèle : 2 et 11 (l'annulée 8 ne compte pas) ; leurs soldes restent à payer.
        let bois = &s.rangees[1];
        assert_eq!((bois.depenses.n, bois.depenses.total, bois.reste), (2, 63_237, 63_237));
        // Bureau Fictif : 10 et 12 (sans total).
        let bureau = &s.rangees[2];
        assert_eq!(
            (bureau.depenses.n, bureau.depenses.total, bureau.depenses.sans_total, bureau.reste),
            (2, 11_498, 1, 11_498)
        );
        // Le TOTAL, et la part de chacun au dixième de pour cent : 195 458 / 270 193 = 72,34 %, 63 237 / 270 193 =
        // 23,40 %, 11 498 / 270 193 = 4,26 %.
        assert_eq!((s.total.depenses.n, s.total.depenses.total, s.total.reste), (7, 270_193, 74_735));
        let parts: Vec<Option<i64>> = s.rangees.iter().map(|l| s.part(l)).collect();
        assert_eq!(parts, [Some(723), Some(234), Some(43)]);
    }

    #[test]
    fn par_fournisseur_suit_les_filtres() {
        // Les douze derniers mois du chantier 7 : les factures 1 et 5 de Béton Fictif.
        let filtres = Filtres { periode: Periode::DouzeMois, chantier: Chantier::Projet(7) };
        let s = par_fournisseur(&registre(), &filtres, AUJOURDHUI);
        assert_eq!(s.rangees.len(), 1);
        assert_eq!((s.rangees[0].nom.as_str(), s.rangees[0].depenses.total), ("Béton Fictif", 160_965));
        assert_eq!(s.part(&s.rangees[0]), Some(1000));
    }

    #[test]
    fn un_fournisseur_ecrit_de_deux_facons() {
        // « BÉTON FICTIF » et « béton-fictif » sont « Béton Fictif » (sans la casse, les accents ni la ponctuation,
        // comme le filtre du tiers de la liste) : une seule rangée, nommée comme sa première facture.
        let mut r = registre();
        let e = Some((7, "Entrepôt"));
        let x = f(
            30,
            Cote::Fournisseur,
            "FACTURE",
            "ENVOYEE",
            Some("2026-10-07"),
            " BÉTON FICTIF ",
            e,
            Some((1_000, 50, 100)),
            1_150,
        );
        let y = f(
            31,
            Cote::Fournisseur,
            "FACTURE",
            "ENVOYEE",
            Some("2026-10-08"),
            "béton-fictif",
            e,
            Some((2_000, 100, 200)),
            2_300,
        );
        r.factures.extend([x, y]);
        let s = par_fournisseur(&r, &tout(), AUJOURDHUI);
        let noms: Vec<&str> = s.rangees.iter().map(|l| l.nom.as_str()).collect();
        assert_eq!(noms, ["Béton Fictif", "Bois Modèle", "Bureau Fictif"]);
        // 195 458 + 1 150 + 2 300 ; les deux nouvelles restent à payer.
        assert_eq!((s.rangees[0].depenses.n, s.rangees[0].depenses.total, s.rangees[0].reste), (5, 198_908, 3_450));
    }

    #[test]
    fn par_chantier_la_marge_avant_taxes() {
        let s = par_chantier(&registre(), &tout(), AUJOURDHUI);
        let noms: Vec<&str> = s.rangees.iter().map(|l| l.nom.as_str()).collect();
        assert_eq!(noms, ["Condos", "Entrepôt", "Sans chantier"], "par leur nom, « Sans chantier » au pied");
        // Les condos : les dépenses 2 et 11 (500 $ + 50 $), le revenu 6 (2 000 $), dont 1 000 $ restent à recevoir.
        let condos = &s.rangees[0];
        assert_eq!(condos.chantier, Chantier::Projet(9));
        assert_eq!((condos.depenses.sous_total, condos.revenus.sous_total), (55_000, 200_000));
        assert_eq!(
            (condos.marge(), condos.taux_de_marge(), condos.a_recevoir, condos.n()),
            (145_000, Some(725), 100_000, 3)
        );
        // L'entrepôt : les dépenses 1, 5 et 13 (1 700 $), les revenus 3 moins la note de crédit 4 (2 800 $) ; la note
        // ne reste pas à recevoir (sa réduction est dans le solde de la facture d'origine).
        let entrepot = &s.rangees[1];
        assert_eq!((entrepot.depenses.sous_total, entrepot.revenus.sous_total), (170_000, 280_000));
        assert_eq!((entrepot.marge(), entrepot.taux_de_marge()), (110_000, Some(393)), "1 100 / 2 800 = 39,29 %");
        assert_eq!((entrepot.a_recevoir, entrepot.n()), (344_925, 5));
        // Sans chantier : les dépenses 10 et 12 ; aucun revenu, donc aucun taux ; la 12 n'a pas de sous-total.
        let sans = &s.rangees[2];
        assert_eq!(sans.chantier, Chantier::Aucun);
        assert_eq!((sans.marge(), sans.taux_de_marge(), sans.sans_sous_total), (-10_000, None, 1));
        // Le TOTAL.
        assert_eq!(s.total.chantier, Chantier::Tous);
        assert_eq!(
            (s.total.depenses.sous_total, s.total.revenus.sous_total, s.total.marge()),
            (235_000, 480_000, 245_000)
        );
        assert_eq!((s.total.a_recevoir, s.total.n(), s.total.sans_sous_total), (444_925, 10, 1));
        assert_eq!(s.total.taux_de_marge(), Some(510), "2 450 / 4 800 = 51,04 %");
    }

    #[test]
    fn une_perte_et_un_nom_qui_manque() {
        // Un chantier sans nom dans l'ERP prend celui du choix d'en tête ; une perte se dit en négatif.
        let mut r = registre();
        let sans_nom = Some((42, ""));
        let x = f(
            20,
            Cote::Fournisseur,
            "FACTURE",
            "ENVOYEE",
            Some("2026-10-01"),
            "Bois Modèle",
            sans_nom,
            Some((40_000, 0, 0)),
            0,
        );
        r.factures.push(x);
        let s = par_chantier(&r, &Filtres { chantier: Chantier::Projet(42), ..tout() }, AUJOURDHUI);
        assert_eq!(s.rangees.len(), 1);
        assert_eq!((s.rangees[0].nom.as_str(), s.rangees[0].marge()), ("Projet 42", -40_000));
        let perte = ChantierLigne {
            revenus: Somme { sous_total: 100_000, ..Somme::default() },
            depenses: Somme { sous_total: 125_000, ..Somme::default() },
            ..ChantierLigne::default()
        };
        assert_eq!(perte.taux_de_marge(), Some(-250));
        assert_eq!(pour_cent(-250), "\u{2212}25,0\u{a0}%");
    }

    #[test]
    fn les_parts_en_dixiemes_de_pour_cent() {
        assert_eq!(dixiemes_de_pour_cent(1, 3), Some(333));
        assert_eq!(dixiemes_de_pour_cent(2, 3), Some(667), "la demie et plus monte");
        assert_eq!(dixiemes_de_pour_cent(1, 8), Some(125), "12,5 % tout rond");
        assert_eq!(dixiemes_de_pour_cent(1, 16), Some(63), "6,25 % : la demie loin de zéro");
        assert_eq!(dixiemes_de_pour_cent(-1, 16), Some(-63));
        assert_eq!(dixiemes_de_pour_cent(5, 0), None);
        assert_eq!(dixiemes_de_pour_cent(5, -10), None);
        assert_eq!(dixiemes_de_pour_cent(i64::MAX, 1), None, "ce qui déborde ne se dit pas");
        assert_eq!(pour_cent(723), "72,3\u{a0}%");
        assert_eq!(pour_cent(1000), "100,0\u{a0}%");
        assert_eq!(pour_cent(5), "0,5\u{a0}%");
    }

    #[test]
    fn vingt_mille_factures_sans_ralentir() {
        // Vingt mille factures sur trois ans, trois cents fournisseurs, quarante chantiers : chaque sommaire en un
        // passage (le seuil, large, attrape un calcul quadratique, pas une machine lente).
        let mut r = Registre::default();
        for i in 0..20_000i64 {
            let cote = if i % 4 == 0 { Cote::Client } else { Cote::Fournisseur };
            let date = format!("{}-{:02}-{:02}", 2024 + (i / 7) % 3, 1 + i % 12, 1 + i % 28);
            let tiers = format!("Tiers {}", i % 300);
            let mut x = f(i, cote, "FACTURE", "ENVOYEE", Some(&date), &tiers, None, Some((10_000, 500, 998)), 11_498);
            x.projet_id = Some(i % 40);
            r.factures.push(x);
        }
        let debut = std::time::Instant::now();
        let mois = par_mois(&r, &tout(), AUJOURDHUI);
        let fournisseurs = par_fournisseur(&r, &tout(), AUJOURDHUI);
        let chantiers = par_chantier(&r, &tout(), AUJOURDHUI);
        assert!(debut.elapsed().as_millis() < 1_000, "{:?}", debut.elapsed());
        assert_eq!((mois.rangees.len(), fournisseurs.rangees.len(), chantiers.rangees.len()), (36, 225, 40));
        assert_eq!(mois.total.depenses.n + mois.total.revenus.n, 20_000);
    }
}
