# -*- coding: utf-8 -*-
r"""
ressources_eve.py — La voix et l'oreille d'Eve : ce que le programme vocal prend sur le poste, téléchargé une fois de
ses sources publiques officielles, chaque fichier vérifié par sa somme SHA-256.

    ressources_eve.py --installer --dossier <commun>          # télécharge ce qui manque (l'installateur, Eve.bat)
    ressources_eve.py --installer --carte-graphique --dossier <commun>
                                                              # et l'oreille d'une carte NVIDIA
    ressources_eve.py --installer --supertonic --dossier <commun>
                                                              # et Supertonic, une voix de plus
    ressources_eve.py --verifier --dossier <commun>           # relit chaque fichier en entier, ne télécharge rien
    ressources_eve.py --liste                                 # la table : adresse, taille, somme

Le kit reste petit : le programme y est, pas ses ressources. Le dossier <commun> est celui que nomment l'installateur et
Eve.bat, %LOCALAPPDATA%\Eve\commun, partagé par tous les spécialistes du poste ; hors OneDrive, et sans accent tant que
le nom de la session Windows n'en a pas (Piper et le serveur de Whisper n'ouvrent pas un chemin accentué) :

        modeles\        Whisper : l'oreille (small pour les demandes, base pour entendre « Eve »)
        voix\           deux voix françaises de Piper (upmc et Siwis), avec leur fiche .json
        piper\piper\    Piper, le moteur de la voix, pour Windows
        whisper-gpu\    seulement sur demande : le serveur de Whisper pour une carte NVIDIA (CUDA 12.4), et
                        large-v3-turbo dans modeles\
        supertonic\     seulement sur demande : Supertonic 2, une voix de plus (le modèle, ses cinq voix de
                        femme, et ONNX Runtime 1.31 dans onnxruntime\)

  - Les sources : Hugging Face (ggerganov/whisper.cpp ; rhasspy/piper-voices, v1.0.0) et les publications GitHub
    officielles (rhasspy/piper 2023.11.14-2 ; ggml-org/whisper.cpp b5454, la v1.9.5). Les adresses de Hugging Face
    passent par son réseau de distribution : la redirection est suivie, la somme fait foi.
  - Un fichier se télécharge d'abord en « .partiel » ; une coupure reprend où elle s'était arrêtée (au relancement
    aussi). Un fichier dont la taille ou la somme ne correspond pas est effacé, et l'erreur le dit : rien de faux ne
    reste sur le poste.
  - Une archive (Piper, le serveur de la carte graphique) se vérifie, puis se déplie à côté et prend sa place d'un coup ;
    elle est ensuite effacée. Le fichier « .source » du dossier déplié garde la somme de l'archive : une nouvelle
    version de la table redéplie.
  - Au lancement, un fichier présent se juge à sa taille (instantané) ; --verifier relit tout, efface ce qui est faux,
    et Eve.bat le retélécharge au lancement suivant.
  - La carte graphique n'est jamais téléchargée d'office : 1,3 Go (le serveur et large-v3-turbo), seulement utiles avec
    une carte NVIDIA. Sans elle, Eve écoute par le processeur.
  - Supertonic non plus : 348 Mo (son modèle, 263 Mo, sur Hugging Face, épinglé sur un commit de son dépôt d'archive ;
    la publication officielle d'ONNX Runtime 1.31, dont on ne garde que la bibliothèque et ses licences). Son modèle est
    sous licence OpenRAIL-M : l'usage commercial est permis, ses restrictions d'usage suivent la voix (son LICENSE est
    téléchargé avec elle).
  - Rien d'autre ne part : une adresse de la table, sans jeton ni nom d'entreprise.

Codes de sortie : 0, tout est là et juste ; 1, un téléchargement ou une vérification a échoué (le message le dit) ;
2, une erreur d'utilisation.
"""
import argparse
import hashlib
import os
import shutil
import sys
import time
import urllib.error
import urllib.request
import zipfile

# Ses sorties en UTF-8 quand elles vont dans un tube (cp1252 sous Windows sans ceci) ; à la console, rien ne change.
for _flux in (sys.stdout, sys.stderr):
    try:
        if not _flux.isatty() and (_flux.encoding or "").lower().replace("-", "") != "utf8":
            _flux.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

# Rien de compilé dans un __pycache__ à côté du script.
sys.dont_write_bytecode = True

HF = "https://huggingface.co"
GH = "https://github.com"

# ---------------------------------------------------------------------------------------------------------------------
# LA TABLE. Pour changer de version : la nouvelle adresse, sa taille et sa somme (vérifiées en la téléchargeant), dans
# un même changement. « dans » : le chemin du fichier dans le dossier commun ; pour une archive, le dossier où elle se
# déplie. « garder » : seulement ces fichiers de l'archive, sous ce préfixe retiré (None : tout, tel quel).
# ---------------------------------------------------------------------------------------------------------------------
RESSOURCES = [
    {
        "nom": "Whisper small (l'oreille, les demandes)",
        "url": f"{HF}/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        "dans": "modeles/ggml-small-q5_1.bin",
        "taille": 190085487,
        "sha256": "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    },
    {
        "nom": "Whisper base (l'oreille, le mot « Eve »)",
        "url": f"{HF}/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        "dans": "modeles/ggml-base-q5_1.bin",
        "taille": 59707625,
        "sha256": "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
    },
    {
        "nom": "Voix upmc",
        "url": f"{HF}/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/upmc/medium/fr_FR-upmc-medium.onnx",
        "dans": "voix/fr_FR-upmc-medium.onnx",
        "taille": 76733615,
        "sha256": "9abb3800c199148897a9ed64e100d224f3de83579f100044174ad19418f1786f",
    },
    {
        "nom": "Voix upmc, sa fiche",
        "url": f"{HF}/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/upmc/medium/fr_FR-upmc-medium.onnx.json",
        "dans": "voix/fr_FR-upmc-medium.onnx.json",
        "taille": 4996,
        "sha256": "e8636ec15dfd5d72db37a02cb5320a20f2b8d339f2a0e4337da64c58a33a5868",
    },
    {
        "nom": "Voix Siwis",
        "url": f"{HF}/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx",
        "dans": "voix/fr_FR-siwis-medium.onnx",
        "taille": 63201294,
        "sha256": "641d1ab097da2b81128c076810edb052b385decc8be3381814802a64a73baf99",
    },
    {
        "nom": "Voix Siwis, sa fiche",
        "url": f"{HF}/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx.json",
        "dans": "voix/fr_FR-siwis-medium.onnx.json",
        "taille": 4875,
        "sha256": "39479916c2db192b5ac9764daddd0c744d83e023ad890c6976c0633ae4df8959",
    },
    {
        # Le fidèle de la carte graphique : large-v3-turbo, aussi vite que small sur une carte, et plus juste (mesuré
        # sur une GTX 1050 Ti : 1,2 s contre 1,4 s pour 10,8 s de parole). Seulement avec la carte : sur le processeur,
        # il serait bien trop lent. Épinglé sur un commit de Hugging Face, pas sur « main ».
        "nom": "Whisper large-v3-turbo (l'oreille de la carte graphique)",
        "url": f"{HF}/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
        "dans": "modeles/ggml-large-v3-turbo-q5_0.bin",
        "taille": 574041195,
        "sha256": "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        "carte_graphique": True,
    },
    {
        # piper.exe et ses bibliothèques, espeak-ng-data : l'archive se déplie dans piper\ (piper\piper\piper.exe).
        # Le runtime Visual C++ n'y est pas : l'installateur et Eve.bat y copient celui du programme.
        "nom": "Piper pour Windows (le moteur de la voix)",
        "url": f"{GH}/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
        "dans": "piper",
        "archive": True,
        "garder": None,
        "taille": 22477236,
        "sha256": "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
        "systeme": "win32",
    },
    {
        # La publication officielle de whisper.cpp pour CUDA 12.4 : on n'en garde que le serveur et ce qu'il charge.
        "nom": "Whisper pour carte NVIDIA (CUDA 12.4)",
        "url": f"{GH}/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-win-cuda-12.4.0-x64.zip",
        "dans": "whisper-gpu",
        "archive": True,
        "garder": ("Release/", [
            "whisper-server.exe", "whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-cuda.dll",
            "ggml-cpu-x64.dll", "ggml-cpu-sse42.dll", "ggml-cpu-sandybridge.dll", "ggml-cpu-haswell.dll",
            "ggml-cpu-skylakex.dll", "ggml-cpu-icelake.dll", "ggml-cpu-cannonlake.dll", "ggml-cpu-cascadelake.dll",
            "ggml-cpu-alderlake.dll", "cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll",
        ]),
        "taille": 684913404,
        "sha256": "afef0b881c500958921c3f5523b50e59ee2ec9b6f5cbd25b324c51ed308a957a",
        "systeme": "win32",
        "carte_graphique": True,
    },
    {
        # La publication officielle d'ONNX Runtime 1.31 (MIT) : Supertonic en a besoin (celui de Piper, de 2023, ne lit
        # pas ses graphes). On n'en garde que la bibliothèque, sa licence et ses avis de tiers.
        "nom": "ONNX Runtime 1.31 (le moteur de Supertonic)",
        "url": f"{GH}/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-win-x64-1.31.0.zip",
        "dans": "supertonic/onnxruntime",
        "archive": True,
        "garder": ("onnxruntime-win-x64-1.31.0/", ["lib/onnxruntime.dll", "LICENSE", "ThirdPartyNotices.txt"]),
        "taille": 82887349,
        "sha256": "3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195",
        "systeme": "win32",
        "supertonic": True,
    },
]

# Supertonic 2 (Supertone : le modèle sous OpenRAIL-M), une voix de plus, seulement sur demande : son dépôt d'archive
# sur Hugging Face, épinglé sur un commit ; ses cinq voix de femme.
SUPERTONIC = f"{HF}/supertone-oss-archive/supertonic-2/resolve/7edbcd0bba8cd579100a353a9e0b2b49cad7c875"
for _nom, _dans, _taille, _somme in [
    ("la durée", "onnx/duration_predictor.onnx", 1521526,
     "6d556b3691165c364be91dc0bd894656b5949f5acd2750d8ec2f954010845011"),
    ("l'encodeur du texte", "onnx/text_encoder.onnx", 27431318,
     "dd5f535ed629f7df86071043e15f541ce1b2ab7f1bdbce4c7892b307bca79fa3"),
    ("le débruitage", "onnx/vector_estimator.onnx", 132471364,
     "105e9d66fd8756876b210a6b4aa03fc393b1eaca3a8dadcc8d9a3bc785c86a35"),
    ("le vocodeur", "onnx/vocoder.onnx", 101405066,
     "19bd51f47a186069c752403518a40f7ea4c647455056d2511f7249691ecddf7c"),
    ("sa fiche", "onnx/tts.json", 8699, "ee531d9af9b80438a2ed703e22155ee6c83b12595ab22fd3bb6de94c7502fe96"),
    ("ses caractères", "onnx/unicode_indexer.json", 262196,
     "b7662a73a0703f43b97c0f2e089f8e8325e26f5d841aca393b5a54c509c92df1"),
    ("sa licence", "LICENSE", 15007, "0d944a9110fed9a9602d60e0423a272903e7bd21ab060490774efc77c2275e9f"),
    ("la voix F1", "voice_styles/F1.json", 420050, "6106950ebeb8a5da29ea22075f605db659cd07dbc288a68292543d9129aa250f"),
    ("la voix F2", "voice_styles/F2.json", 420293, "8b97feb16d79ac0447136796708feac5f83dbabe92a5be1168212653c38729ae"),
    ("la voix F3", "voice_styles/F3.json", 419887, "7eda5bccb4e6eb7f228fa182462d5fcf982d77628234603599027f0734d70c29"),
    ("la voix F4", "voice_styles/F4.json", 419642, "e056fc2bee393edc8bff761eb28f33fb461e8dad828c3b05348a010ac1b7bb79"),
    ("la voix F5", "voice_styles/F5.json", 419709, "ce7645ad7e3c13cca04e0d62bf890ef9ac401988005ba8f5e9c9b59257bc6931"),
]:
    RESSOURCES.append({"nom": f"Supertonic 2, {_nom}", "url": f"{SUPERTONIC}/{_dans}", "dans": f"supertonic/{_dans}",
                       "taille": _taille, "sha256": _somme, "supertonic": True})

ESSAIS = 5            # tentatives par fichier, chacune reprend où la précédente s'est arrêtée
DELAI_RESEAU = 30     # secondes sans un octet : la tentative s'arrête
BLOC = 1 << 20
AGENT = "Eve (ressources_eve.py)"
MO = 1_000_000


class Echec(Exception):
    """Une erreur à dire telle quelle à la personne."""


def ressources(carte_graphique=False, supertonic=False):
    """Les ressources de ce système ; celles de la carte graphique et de Supertonic seulement si on les demande."""
    return [r for r in RESSOURCES
            if r.get("systeme", sys.platform) == sys.platform
            and (carte_graphique or not r.get("carte_graphique"))
            and (supertonic or not r.get("supertonic"))]


def chemin(local, r):
    return os.path.join(local, *r["dans"].split("/"))


def marque(local, r):
    return os.path.join(chemin(local, r), ".source")


def presente(local, r):
    """Au lancement : la taille pour un fichier, la marque de l'archive pour un dossier déplié. Instantané."""
    if r.get("archive"):
        try:
            with open(marque(local, r), encoding="ascii") as f:
                return f.read().strip() == r["sha256"]
        except OSError:
            return False
    try:
        return os.path.getsize(chemin(local, r)) == r["taille"]
    except OSError:
        return False


def somme(fichier):
    h = hashlib.sha256()
    with open(fichier, "rb") as f:
        for bloc in iter(lambda: f.read(BLOC), b""):
            h.update(bloc)
    return h.hexdigest()


def mo(n):
    return f"{n / MO:,.0f}".replace(",", " ")


class Progression:
    """Une barre sur une ligne, à la console ; dans un tube, une ligne par fichier fini."""

    def __init__(self, total):
        self.total = max(total, 1)
        self.fait = 0
        self.console = sys.stdout.isatty()
        self.dernier = 0.0

    def avancer(self, n, force=False):
        self.fait += n
        if not self.console:
            return
        maintenant = time.monotonic()
        if not force and maintenant - self.dernier < 0.25:
            return
        self.dernier = maintenant
        part = min(self.fait / self.total, 1.0)
        plein = int(part * 30)
        sys.stdout.write(f"\r   [{'#' * plein}{'.' * (30 - plein)}] {part * 100:3.0f} %  "
                         f"{mo(self.fait)} / {mo(self.total)} Mo   ")
        sys.stdout.flush()

    def ligne(self, texte):
        if self.console:
            sys.stdout.write("\r" + " " * 70 + "\r")
        print(f"   {texte}")
        self.avancer(0, force=True)

    def fin(self):
        if self.console:
            sys.stdout.write("\n")
            sys.stdout.flush()


def telecharger(r, cible, prog):
    """Télécharge r["url"] dans cible (reprise d'un .partiel), vérifie la taille et la somme, puis le met en place."""
    partiel = cible + ".partiel"
    os.makedirs(os.path.dirname(cible), exist_ok=True)
    taille = r["taille"]
    deja = os.path.getsize(partiel) if os.path.exists(partiel) else 0
    if deja > taille:
        os.remove(partiel)
        deja = 0
    prog.avancer(deja)
    derniere_erreur = None
    for essai in range(ESSAIS):
        if deja >= taille:
            break
        if essai:
            time.sleep(min(2 * essai, 10))
        req = urllib.request.Request(r["url"], headers={"User-Agent": AGENT})
        if deja:
            req.add_header("Range", f"bytes={deja}-")
        try:
            with urllib.request.urlopen(req, timeout=DELAI_RESEAU) as rep:
                if deja and rep.status != 206:
                    # Le serveur repart du début : on aussi.
                    prog.avancer(-deja)
                    deja = 0
                with open(partiel, "ab" if deja else "wb") as f:
                    while True:
                        bloc = rep.read(BLOC)
                        if not bloc:
                            break
                        if deja + len(bloc) > taille:
                            raise Echec(f"{r['nom']} : le serveur envoie plus que les {mo(taille)} Mo attendus.")
                        f.write(bloc)
                        deja += len(bloc)
                        prog.avancer(len(bloc))
        except Echec:
            _effacer(partiel)
            raise
        except (urllib.error.URLError, OSError, ValueError) as e:
            derniere_erreur = e
            if isinstance(e, urllib.error.HTTPError) and e.code in (403, 404, 410):
                break
            if isinstance(e, urllib.error.HTTPError) and e.code == 416:
                _effacer(partiel)
                prog.avancer(-deja)
                deja = 0
            continue
    if deja < taille and isinstance(derniere_erreur, urllib.error.HTTPError) and derniere_erreur.code in (403, 404, 410):
        raise Echec(f"{r['nom']} : la source ne le donne plus ({_raison(derniere_erreur)}, {r['url']}). "
                    f"Le kit d'Eve est à mettre à jour : en demander la dernière version à qui l'a donné.")
    if deja < taille:
        raise Echec(f"{r['nom']} : le téléchargement n'a pas abouti ({_raison(derniere_erreur)}). "
                    f"Ce qui est reçu est gardé : relancer Eve.bat reprendra là.")
    trouvee = somme(partiel)
    if trouvee != r["sha256"]:
        _effacer(partiel)
        raise Echec(f"{r['nom']} : le fichier reçu ne correspond pas à sa somme SHA-256 "
                    f"(attendue {r['sha256']}, reçue {trouvee}). Il est effacé ; rien n'est installé. "
                    f"Relancer Eve.bat le retéléchargera ; si cela se répète, le réseau du poste modifie les "
                    f"fichiers (un pare-feu, un proxy) ou la source a changé.")
    os.replace(partiel, cible)


def _raison(e):
    if e is None:
        return "aucune réponse"
    if isinstance(e, urllib.error.HTTPError):
        return f"le serveur répond {e.code}"
    if isinstance(e, urllib.error.URLError):
        return f"réseau : {e.reason}"
    return str(e)


def _effacer(p):
    try:
        if os.path.isdir(p):
            shutil.rmtree(p)
        elif os.path.exists(p):
            os.remove(p)
    except OSError:
        pass


def deplier(r, archive, local):
    """Déplie l'archive vérifiée à côté de sa place, puis l'y met d'un coup ; la marque garde sa somme."""
    final = chemin(local, r)
    neuf = final + ".nouveau"
    _effacer(neuf)
    os.makedirs(neuf)
    garder = r.get("garder")
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            nom = info.filename
            if info.is_dir():
                continue
            if garder:
                prefixe, noms = garder
                if not nom.startswith(prefixe) or nom[len(prefixe):] not in noms:
                    continue
                nom = nom[len(prefixe):]
            parties = [p for p in nom.replace("\\", "/").split("/") if p]
            if not parties or any(p in (".", "..") or ":" in p for p in parties) or nom.startswith("/"):
                raise Echec(f"{r['nom']} : l'archive contient un chemin refusé ({info.filename}).")
            dest = os.path.join(neuf, *parties)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with z.open(info) as src, open(dest, "wb") as out:
                shutil.copyfileobj(src, out, BLOC)
    if garder:
        manquants = [n for n in garder[1] if not os.path.isfile(os.path.join(neuf, n))]
        if manquants:
            _effacer(neuf)
            raise Echec(f"{r['nom']} : l'archive n'a pas {', '.join(manquants)}.")
    with open(os.path.join(neuf, ".source"), "w", encoding="ascii") as f:
        f.write(r["sha256"] + "\n")
    _effacer(final)
    os.replace(neuf, final)


def installer(local, carte_graphique, supertonic=False):
    voulues = ressources(carte_graphique, supertonic)
    manquantes = [r for r in voulues if not presente(local, r)]
    if not manquantes:
        return 0
    total = sum(r["taille"] for r in manquantes)
    combien = f"environ {mo(total)} Mo" if total >= MO else "moins d'un Mo"
    premiere = not os.path.isdir(os.path.join(local, "modeles"))
    print()
    if premiere:
        print(f"   Première fois sur ce poste : je télécharge la voix et l'oreille d'Eve, {combien}.")
        print("   Quelques minutes selon la connexion ; les fois suivantes, rien à télécharger.")
    else:
        print(f"   Il manque {len(manquantes)} fichier(s) d'Eve sur ce poste : je les télécharge, "
              f"{combien}.")
    print(f"   Dans {local}")
    os.makedirs(local, exist_ok=True)
    try:
        libre = shutil.disk_usage(local).free
    except OSError:
        libre = None
    besoin = sum(r["taille"] * (3 if r.get("archive") else 1) for r in manquantes)
    if libre is not None and libre < besoin:
        print(f"\n   Pas assez de place sur le disque : il faut environ {mo(besoin)} Mo, il en reste {mo(libre)}.")
        return 1
    prog = Progression(total)
    try:
        for r in manquantes:
            if r.get("archive"):
                archive = os.path.join(local, "telechargements", os.path.basename(r["url"]))
                telecharger(r, archive, prog)
                deplier(r, archive, local)
                _effacer(archive)
            else:
                telecharger(r, chemin(local, r), prog)
            prog.ligne(f"[ OK ] {r['nom']}")
    except Echec as e:
        prog.fin()
        print(f"\n   [ECHEC] {e}")
        return 1
    except OSError as e:
        prog.fin()
        print(f"\n   [ECHEC] Écriture impossible dans {local} : {e}")
        return 1
    prog.fin()
    _effacer(os.path.join(local, "telechargements"))
    print("   Tout est là, et vérifié.")
    return 0


def verifier(local, carte_graphique, supertonic=False):
    """Relit chaque fichier en entier. Un fichier faux est effacé : le prochain lancement le retélécharge."""
    voulues = ressources(carte_graphique or os.path.isdir(os.path.join(local, "whisper-gpu")),
                         supertonic or os.path.isdir(os.path.join(local, "supertonic")))
    print(f"   Dossier commun : {local}")
    mauvais = 0
    for r in voulues:
        p = chemin(local, r)
        if r.get("archive"):
            ok = presente(local, r)
            etat = "déplié" if ok else "absent ou d'une autre version"
        elif not os.path.isfile(p):
            ok, etat = False, "absent"
        elif os.path.getsize(p) != r["taille"]:
            ok, etat = False, "taille fausse : effacé"
            _effacer(p)
        elif somme(p) != r["sha256"]:
            ok, etat = False, "somme SHA-256 fausse : effacé"
            _effacer(p)
        else:
            ok, etat = True, "somme vérifiée"
        mauvais += not ok
        print(f"   {'[ OK ]' if ok else '[ECHEC]'} {r['nom']} — {etat}")
    if mauvais:
        print("   Relancer Eve.bat : il retéléchargera ce qui manque.")
    return 1 if mauvais else 0


def liste():
    for r in RESSOURCES:
        sorte = "archive" if r.get("archive") else "fichier"
        print(f"{r['dans']}  ({sorte}, {r['taille']} octets, {r.get('systeme', 'tous')})\n"
              f"    {r['url']}\n    sha256 {r['sha256']}")
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(description="Les ressources d'Eve : la voix et l'oreille, téléchargées et vérifiées.")
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--installer", action="store_true", help="télécharge ce qui manque")
    g.add_argument("--verifier", action="store_true", help="relit chaque fichier en entier")
    g.add_argument("--liste", action="store_true", help="la table des ressources")
    p.add_argument("--carte-graphique", action="store_true",
                   help="aussi l'oreille d'une carte NVIDIA : le serveur de Whisper et large-v3-turbo (1,3 Go)")
    p.add_argument("--supertonic", action="store_true",
                   help="aussi Supertonic, une voix de plus : son modèle et ONNX Runtime (348 Mo)")
    p.add_argument("--dossier", help="le dossier commun de la voix et de l'oreille (%%LOCALAPPDATA%%\\Eve\\commun)")
    a = p.parse_args(argv)
    if a.liste:
        return liste()
    if not a.dossier:
        p.error("--dossier manque : le dossier commun de la voix et de l'oreille")
    local = os.path.abspath(a.dossier)
    if sys.platform != "win32":
        print("   La voix d'Eve n'est encore offerte que sous Windows : sur ce poste, Eve s'utilise au clavier.",
              file=sys.stderr)
        return 1
    if a.verifier:
        return verifier(local, a.carte_graphique, a.supertonic)
    return installer(local, a.carte_graphique, a.supertonic)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\n   Interrompu : ce qui est reçu est gardé, relancer Eve.bat reprendra là.", file=sys.stderr)
        sys.exit(1)
