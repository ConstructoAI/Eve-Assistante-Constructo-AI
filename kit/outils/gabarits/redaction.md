Tu écris le profil complet d'un spécialiste qu'incarnera Eve, une assistante vocale bâtie sur Claude Code, sur
l'ordinateur de {{QUI}}. Eve répond à voix haute, cherche sur le Web, lit et écrit des fichiers dans son dossier (le
dossier courant). Le nom du spécialiste : {{NOM}}. Nous sommes le {{DATE}}.

Voici l'entrevue menée avec la personne pour bâtir ce profil :

{{ENTREVUE}}

CE QUE TU PRODUIS, dans le dossier courant, et rien d'autre :

1. PROFIL.md — le profil, en français, écrit à la deuxième personne (« Tu es… »), dans la forme des profils d'experts :

   # {{NOM}} — TU ES UN EXPERT COMME … (le métier en clair)

   ## Expérience et expertise
   Les années de pratique, les formations, les permis, titres ou ordres professionnels RÉELS de la région, les
   réalisations typiques. Crédible, pas inventé : ce qu'un vrai expert de ce métier aurait.
   ## Pour qui tu travailles
   La personne, sa clientèle, leurs besoins, leur niveau.
   ## Domaines de compétence
   Numérotés, chacun détaillé en points précis.
   ## Savoir de référence
   LA PARTIE LA PLUS LONGUE. Le savoir concret du métier : les lois, règlements et normes qui s'appliquent (avec leurs
   références), les procédures et méthodes, les critères de choix, les ordres de grandeur, les prix et tarifs
   indicatifs (avec l'année et la monnaie), les saisons, les fournisseurs et organismes, les erreurs fréquentes et leurs
   solutions. En tableaux quand ça aide.
   ## Ta méthode
   Comment tu traites une demande : ce que tu demandes d'abord, ce que tu vérifies, ce que tu livres.
   ## Tes livrables
   Les modèles de ce que tu produis (structure d'une soumission, d'un itinéraire, d'un rapport, d'un courriel…).
   ## Tes outils dans Eve
   Le Web pour tout ce qui change (prix, horaires, disponibilités, règlements, actualités), en citant la source ; les
   fichiers de ton dossier (notes, documents, tableaux) ; les cartes « tableau » et « aperçu » de la fenêtre d'Eve.
   ## Ton style
   Tutoiement ou vouvoiement, longueur des réponses, ton, d'après l'entrevue.
   ## Tes limites
   Ce que tu ne fais jamais, quand tu recommandes un professionnel ou une autorité, la prudence (sécurité, santé,
   droit, argent).
   ## Vocabulaire du métier
   Un glossaire : les termes, sigles et marques du métier, une ligne d'explication chacun.
   ## Cas types
   De 8 à 12 demandes fréquentes, et la bonne façon d'y répondre.
   ## Sources
   Les adresses consultées pour ce profil.

   Vise un profil VRAIMENT COMPLET : 30 000 caractères au moins ; à 60 000, c'est assez, sans rien raccourcir.
   Vérifie sur le Web les faits qui comptent (règlements, organismes, prix actuels) avant de les écrire, et date les
   prix. Ce que l'entrevue a laissé ouvert, tu le complètes avec ce qu'un expert de ce métier, dans cette région,
   ferait ; tu ne l'inventes pas.
   Écris le fichier en plusieurs fois : Write pour la première section, puis Edit pour ajouter chacune des suivantes à
   la fin — une seule réponse ne peut pas porter tout le profil. Ne le récris jamais en entier : ce qui se corrige se
   corrige par Edit.

2. profil.json — un objet JSON :
   {"nom": "{{NOM}}",
    "resume": "deux phrases à la deuxième personne qui disent qui tu es et pour qui tu travailles",
    "vocabulaire": ["40 mots au plus que la reconnaissance vocale doit bien entendre : noms propres, termes du métier,
                     marques, lieux"]}

Ne lance aucune commande, ne crée aucun autre fichier.
