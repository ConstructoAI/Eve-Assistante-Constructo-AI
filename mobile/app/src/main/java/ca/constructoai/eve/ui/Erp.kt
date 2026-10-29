package ca.constructoai.eve.ui

import androidx.compose.ui.graphics.Color

/**
 * La palette de l'ERP de Constructo AI, celle du thème clair de la fenêtre d'Eve sur l'ordinateur : les teintes du module
 * `erp` de `src\natif\theme.rs`, recopiées ici (Kotlin ne lit pas le Rust) — une teinte qui change là change ici.
 */
object Erp {
    /** Le bleu de Constructo AI et son appui. */
    val Primaire = Color(0xFF0078D4)
    val PrimaireFonce = Color(0xFF005EA2)

    /** Le marine de la barre, de haut en bas. */
    val MarineHaut = Color(0xFF002B6B)
    val Marine = Color(0xFF002050)

    /** Le fond qui respire, entre des blancs. */
    val Fond1 = Color(0xFFBFDBFE)
    val Fond2 = Color(0xFFDBEAFE)

    /** Les encres de Fluent : le texte, le secondaire, le discret. */
    val Texte = Color(0xFF323130)
    val Texte2 = Color(0xFF605E5C)
    val Texte3 = Color(0xFFA19F9D)

    /** Le titre d'une carte, le filet froid des cartes, le texte des réponses. */
    val Titre = Color(0xFF1F2937)
    val FiletDeCarte = Color(0xFFE5EAF1)
    val Ardoise = Color(0xFF334155)

    /** Le bleu vif de la bannière, de son milieu à son bout. */
    val Azur = Color(0xFF0B4AA6)
    val Ciel = Color(0xFF1E88E5)

    /** Les bulles : le liseré de celles d'Eve, le dégradé de celles de la personne. */
    val BulleLisere = Color(0xFF3B82F6)
    val Toi = listOf(Color(0xFF2563EB), Color(0xFF1D4ED8))

    /** Les états. */
    val Succes = Color(0xFF22C55E)
    val Avertissement = Color(0xFFF59E0B)
    val DangerEncre = Color(0xFF991B1B)
}
