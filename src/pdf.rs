//! Les numérisations d'Eve en PDF : les pages photographiées par la caméra (des JPEG que la page prépare : recadrées,
//! redressées, blanchies) réunies en un PDF, une page par image. Le JPEG entre tel quel dans le PDF (le filtre
//! `DCTDecode` des lecteurs le décode) : rien n'est réencodé, rien ne se perd.
//!
//! La page envoie ses pages en une seule requête, chacune précédée de sa longueur sur quatre octets (gros-boutiste) :
//! [`lire_trames`] les sépare, [`jpeg`] lit la taille de chaque image dans son en-tête, et [`assembler`] écrit le PDF.

/// Au plus tant de pages par numérisation.
pub const PAGES_MAX: usize = 40;
/// Le grand côté d'une page du PDF : onze pouces (792 points), comme une feuille Lettre.
const GRAND_COTE: f64 = 792.0;

/// Une image JPEG, lue dans son en-tête.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jpeg {
    pub largeur: u32,
    pub hauteur: u32,
    /// 1 : gris ; 3 : couleur ; 4 : CMJN.
    pub composantes: u8,
}

/// Les pages d'une requête : chacune précédée de sa longueur (quatre octets, gros-boutiste).
pub fn lire_trames(corps: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut pages = Vec::new();
    let mut i = 0;
    while i < corps.len() {
        let Some(tete) = corps.get(i..i + 4) else { return Err("une longueur de page est tronquée".into()) };
        let n = u32::from_be_bytes([tete[0], tete[1], tete[2], tete[3]]) as usize;
        i += 4;
        if n == 0 {
            return Err("une page est vide".into());
        }
        let Some(page) = corps.get(i..i.saturating_add(n)) else { return Err("une page est tronquée".into()) };
        pages.push(page);
        i += n;
        if pages.len() > PAGES_MAX {
            return Err(format!("plus de {PAGES_MAX} pages"));
        }
    }
    if pages.is_empty() {
        return Err("aucune page".into());
    }
    Ok(pages)
}

/// La taille d'une image JPEG, lue dans son segment SOF ; `None` si ce n'est pas un JPEG lisible.
pub fn jpeg(o: &[u8]) -> Option<Jpeg> {
    if o.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    loop {
        if *o.get(i)? != 0xFF {
            return None;
        }
        // Les octets de remplissage (0xFF répétés) avant le marqueur.
        while *o.get(i)? == 0xFF {
            i += 1;
        }
        let marqueur = *o.get(i)?;
        i += 1;
        match marqueur {
            // Sans longueur : TEM et les marqueurs de redémarrage.
            0x01 | 0xD0..=0xD7 => continue,
            // La fin de l'image, ou le début des données, avant tout SOF : pas de taille à lire.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let longueur = u16::from_be_bytes([*o.get(i)?, *o.get(i + 1)?]) as usize;
        if longueur < 2 {
            return None;
        }
        // Les SOF : C0 à CF, sauf C4 (les tables de Huffman), C8 (réservé) et CC (le codage arithmétique).
        if matches!(marqueur, 0xC0..=0xCF) && !matches!(marqueur, 0xC4 | 0xC8 | 0xCC) {
            // Après la longueur : la précision, la hauteur, la largeur, le nombre de composantes.
            let s = o.get(i + 2..i + 8)?;
            let hauteur = u16::from_be_bytes([s[1], s[2]]) as u32;
            let largeur = u16::from_be_bytes([s[3], s[4]]) as u32;
            let composantes = s[5];
            if largeur == 0 || hauteur == 0 || !matches!(composantes, 1 | 3 | 4) {
                return None;
            }
            return Some(Jpeg { largeur, hauteur, composantes });
        }
        i += longueur;
    }
}

/// Le PDF des pages : une page par image, au format de l'image (son grand côté à onze pouces). `titre` et `date`
/// (« D:20261022101502 ») vont dans ses propriétés.
pub fn assembler(pages: &[&[u8]], titre: &str, date: &str) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("aucune page".into());
    }
    let images: Vec<Jpeg> = pages
        .iter()
        .enumerate()
        .map(|(i, p)| jpeg(p).ok_or_else(|| format!("la page {} n'est pas une image JPEG", i + 1)))
        .collect::<Result<_, _>>()?;
    let n = pages.len();
    // Les objets : 1 le catalogue, 2 l'arbre des pages, 3 les propriétés ; puis, par page, la page, son image et ce
    // qui la dessine.
    let total = 4 + 3 * n;
    let mut positions = vec![0usize; total];
    let mut sortie: Vec<u8> = Vec::with_capacity(pages.iter().map(|p| p.len()).sum::<usize>() + 1024 * (n + 1));
    sortie.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let pages_filles: Vec<String> = (0..n).map(|k| format!("{} 0 R", 4 + 3 * k)).collect();
    objet(&mut sortie, &mut positions, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    objet(
        &mut sortie,
        &mut positions,
        2,
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", pages_filles.join(" ")).as_bytes(),
    );
    let date: String = date.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ':').collect();
    objet(
        &mut sortie,
        &mut positions,
        3,
        format!("<< /Title {} /Producer (Eve - Entrepreneur General) /CreationDate ({date}) >>", texte_pdf(titre))
            .as_bytes(),
    );
    for (k, (page, image)) in pages.iter().zip(&images).enumerate() {
        let (numero_page, numero_image, numero_dessin) = (4 + 3 * k, 5 + 3 * k, 6 + 3 * k);
        let (largeur, hauteur) = format_de_page(image);
        let (l, h) = (nombre(largeur), nombre(hauteur));
        objet(
            &mut sortie,
            &mut positions,
            numero_page,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {l} {h}] /Resources << /XObject << /Im0 {numero_image} 0 R >> >> \
                 /Contents {numero_dessin} 0 R >>"
            )
            .as_bytes(),
        );
        let espace = match image.composantes {
            1 => "/DeviceGray",
            // Les JPEG en CMJN (ceux d'Adobe) sont inversés.
            4 => "/DeviceCMYK /Decode [1 0 1 0 1 0 1 0]",
            _ => "/DeviceRGB",
        };
        let mut corps = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {espace} /BitsPerComponent 8 \
             /Filter /DCTDecode /Length {} >>\nstream\n",
            image.largeur,
            image.hauteur,
            page.len()
        )
        .into_bytes();
        corps.extend_from_slice(page);
        corps.extend_from_slice(b"\nendstream");
        objet(&mut sortie, &mut positions, numero_image, &corps);
        let dessin = format!("q {l} 0 0 {h} 0 0 cm /Im0 Do Q");
        objet(
            &mut sortie,
            &mut positions,
            numero_dessin,
            format!("<< /Length {} >>\nstream\n{dessin}\nendstream", dessin.len()).as_bytes(),
        );
    }
    let xref = sortie.len();
    sortie.extend_from_slice(format!("xref\n0 {total}\n0000000000 65535 f \n").as_bytes());
    for position in &positions[1..] {
        // Une entrée fait exactement vingt octets, fin de ligne comprise.
        sortie.extend_from_slice(format!("{position:010} 00000 n \n").as_bytes());
    }
    sortie.extend_from_slice(
        format!("trailer\n<< /Size {total} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Ok(sortie)
}

fn objet(sortie: &mut Vec<u8>, positions: &mut [usize], numero: usize, corps: &[u8]) {
    positions[numero] = sortie.len();
    sortie.extend_from_slice(format!("{numero} 0 obj\n").as_bytes());
    sortie.extend_from_slice(corps);
    sortie.extend_from_slice(b"\nendobj\n");
}

/// La page d'une image, en points : son grand côté à onze pouces, l'autre à proportion.
fn format_de_page(image: &Jpeg) -> (f64, f64) {
    let (l, h) = (image.largeur as f64, image.hauteur as f64);
    if h >= l { (GRAND_COTE * l / h, GRAND_COTE) } else { (GRAND_COTE, GRAND_COTE * h / l) }
}

/// Un nombre du PDF : deux décimales au plus, sans zéro inutile (« 612 », « 618.75 »).
fn nombre(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Une chaîne du PDF en UTF-16 (gros-boutiste, avec sa marque), écrite en hexadécimal : les accents passent, et rien
/// n'est à échapper.
fn texte_pdf(s: &str) -> String {
    let mut h = String::from("<FEFF");
    for u in s.encode_utf16() {
        h.push_str(&format!("{u:04X}"));
    }
    h.push('>');
    h
}

#[cfg(test)]
mod essais {
    use super::*;

    /// L'en-tête d'un JPEG, sans image : SOI, APP0 (JFIF), un octet de remplissage, SOF0, EOI.
    fn faux_jpeg(largeur: u16, hauteur: u16, composantes: u8) -> Vec<u8> {
        let mut o = vec![0xFF, 0xD8];
        o.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]);
        o.extend_from_slice(&[0xFF, 0xFF, 0xC0, 0x00, 8 + 3 * composantes, 8]);
        o.extend_from_slice(&hauteur.to_be_bytes());
        o.extend_from_slice(&largeur.to_be_bytes());
        o.push(composantes);
        for c in 0..composantes {
            o.extend_from_slice(&[c + 1, 0x11, 0]);
        }
        o.extend_from_slice(&[0xFF, 0xD9]);
        o
    }

    fn trames(pages: &[&[u8]]) -> Vec<u8> {
        let mut o = Vec::new();
        for p in pages {
            o.extend_from_slice(&(p.len() as u32).to_be_bytes());
            o.extend_from_slice(p);
        }
        o
    }

    #[test]
    fn lire_la_taille_d_un_jpeg() {
        assert_eq!(jpeg(&faux_jpeg(200, 256, 3)), Some(Jpeg { largeur: 200, hauteur: 256, composantes: 3 }));
        assert_eq!(jpeg(&faux_jpeg(1700, 2200, 1)), Some(Jpeg { largeur: 1700, hauteur: 2200, composantes: 1 }));
        let mut progressif = faux_jpeg(640, 480, 3);
        let sof = progressif.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        progressif[sof + 1] = 0xC2;
        assert_eq!(jpeg(&progressif).map(|j| (j.largeur, j.hauteur)), Some((640, 480)));
        assert_eq!(jpeg(b"%PDF-1.4"), None);
        assert_eq!(jpeg(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
        assert_eq!(jpeg(&faux_jpeg(200, 256, 3)[..20]), None);
        assert_eq!(jpeg(&faux_jpeg(0, 256, 3)), None);
    }

    #[test]
    fn separer_les_pages() {
        let (a, b) = (faux_jpeg(10, 20, 3), faux_jpeg(30, 40, 1));
        let corps = trames(&[&a, &b]);
        let pages = lire_trames(&corps).unwrap();
        assert_eq!(pages, vec![a.as_slice(), b.as_slice()]);
        assert!(lire_trames(&[]).is_err());
        assert!(lire_trames(&corps[..corps.len() - 1]).is_err());
        assert!(lire_trames(&corps[..2]).is_err());
        assert!(lire_trames(&[0, 0, 0, 0]).is_err());
        let trop: Vec<&[u8]> = (0..=PAGES_MAX).map(|_| a.as_slice()).collect();
        assert!(lire_trames(&trames(&trop)).is_err());
        assert!(lire_trames(&[0xFF, 0xFF, 0xFF, 0xFF, 1]).is_err());
    }

    #[test]
    fn assembler_un_pdf() {
        let (a, b) = (faux_jpeg(1700, 2200, 3), faux_jpeg(2200, 1000, 1));
        let pdf = assembler(&[&a, &b], "Numérisation d'Eve", "D:20261022101502").unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let texte = String::from_utf8_lossy(&pdf);
        assert!(texte.contains("/Count 2"));
        assert!(texte.contains("/MediaBox [0 0 612 792]"));
        assert!(texte.contains("/MediaBox [0 0 792 360]"));
        assert!(texte.contains("/ColorSpace /DeviceRGB"));
        assert!(texte.contains("/ColorSpace /DeviceGray"));
        assert!(texte.contains("/CreationDate (D:20261022101502)"));
        // « é » en UTF-16 : 00E9.
        assert!(texte.contains("<FEFF004E0075006D00E9"));
        // Les images y sont telles quelles.
        assert!(pdf.windows(a.len()).any(|w| w == a.as_slice()));
        assert!(pdf.windows(b.len()).any(|w| w == b.as_slice()));
        // La table des renvois : chaque entrée vise bien « k 0 obj », en octets (le texte ci-dessus, lu avec pertes,
        // n'a plus les mêmes positions).
        let marque = b"startxref\n";
        let debut = pdf.windows(marque.len()).rposition(|w| w == marque).unwrap() + marque.len();
        let fin = debut + pdf[debut..].iter().position(|&o| o == b'\n').unwrap();
        let xref: usize = std::str::from_utf8(&pdf[debut..fin]).unwrap().parse().unwrap();
        let table = std::str::from_utf8(&pdf[xref..]).unwrap();
        assert!(table.starts_with("xref\n0 10\n0000000000 65535 f \n"));
        let entrees: Vec<&str> = table.split('\n').skip(3).take(9).collect();
        for (k, e) in entrees.iter().enumerate() {
            assert_eq!(e.len(), 19, "« {e} » (vingt octets avec le saut de ligne)");
            let position: usize = e[..10].parse().unwrap();
            assert!(pdf[position..].starts_with(format!("{} 0 obj\n", k + 1).as_bytes()), "objet {}", k + 1);
        }
    }

    #[test]
    fn refuser_ce_qui_n_est_pas_une_image() {
        assert!(assembler(&[], "x", "D:2026").is_err());
        assert!(assembler(&[b"pas une image"], "x", "D:2026").is_err());
    }

    #[test]
    fn des_nombres_et_des_chaines() {
        assert_eq!(nombre(612.0), "612");
        assert_eq!(nombre(618.75), "618.75");
        assert_eq!(nombre(360.004), "360");
        assert_eq!(texte_pdf("Ève"), "<FEFF00C800760065>");
    }
}
