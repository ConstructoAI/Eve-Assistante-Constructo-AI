package ca.constructoai.eve.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ca.constructoai.eve.EtatDeLaLiaison
import ca.constructoai.eve.Ligne
import ca.constructoai.eve.Modele
import ca.constructoai.eve.R

/**
 * La conversation, comme le dock de l'assistant de l'ERP et la conversation de la fenêtre de l'ordinateur : la barre
 * marine, le visage d'Eve et ce qu'elle fait, les bulles (celles d'Eve blanches au liseré bleu, celles de la personne en
 * dégradé bleu), la réponse qui s'écrit, l'accord qu'elle attend ; en bas, la saisie, le trombone et le micro à tenir.
 * Les icônes sont celles de Material Icons (Apache 2.0).
 */
@Composable
fun Conversation(m: Modele, choisirUnePhoto: () -> Unit) {
    val eve = m.eve
    FondQuiRespire {
        Column(Modifier.fillMaxSize().imePadding()) {
            BarreDeLaConversation(m)
            EtatDEve(m)
            val defile = rememberLazyListState()
            val lignes = eve.fil.filter { it.de != "systeme" || it.texte.isNotBlank() }
            LaunchedEffect(lignes.size, eve.enCours?.length, eve.accord?.numero) {
                val n = lignes.size + (if (eve.enCours != null) 1 else 0) + (if (eve.accord != null) 1 else 0)
                if (n > 0) defile.animateScrollToItem(n - 1)
            }
            LazyColumn(
                state = defile,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(lignes) { Bulle(it) }
                eve.enCours?.let { texte -> item { Bulle(Ligne("eve", texte, "", emptyList(), ""), enCours = true) } }
                eve.accord?.let { a ->
                    item {
                        CarteDAccord(a.quoi, a.detail, confirmer = { m.confirmer(a) }, annuler = { m.annuler(a) })
                    }
                }
            }
            m.message?.let { Message(it, m::oublierLeMessage) }
            if (eve.pieces.isNotEmpty()) PiecesJointes(m)
            Saisie(m, choisirUnePhoto)
        }
    }
}

@Composable
private fun BarreDeLaConversation(m: Modele) {
    val j = m.jumelage
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier
            .fillMaxWidth()
            .background(Brush.verticalGradient(listOf(Erp.MarineHaut, Erp.Marine)))
            .statusBarsPadding()
            .padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(painterResource(R.drawable.logo_blanc), contentDescription = "Constructo AI", Modifier.size(28.dp))
        Spacer(Modifier.size(10.dp))
        Column(Modifier.weight(1f)) {
            Text("EVE", color = Color.White, fontWeight = FontWeight.Bold, fontSize = 17.sp, letterSpacing = 1.sp)
            val qui = m.eve.qui.ifBlank { j?.qui.orEmpty() }
            val sous = listOfNotNull(j?.eve, qui.takeIf(String::isNotBlank)).joinToString(" · ")
            Text(sous, color = Color(0xCCFFFFFF), fontSize = 12.sp, maxLines = 1)
        }
        PastilleDeLaLiaison(m.liaison)
        Box {
            IconButton(onClick = { menu = true }) {
                Icon(painterResource(R.drawable.ic_menu), contentDescription = "Plus", tint = Color.White)
            }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(text = { Text("Nouvelle conversation") }, onClick = { menu = false; m.nouvelle() })
                DropdownMenuItem(
                    text = { Text(if (m.sourdine) "Entendre Eve ici" else "Sourdine sur ce téléphone") },
                    onClick = { menu = false; m.basculerLaSourdine() },
                )
                HorizontalDivider()
                DropdownMenuItem(
                    text = { Text("Oublier ce jumelage", color = Erp.DangerEncre) },
                    onClick = { menu = false; m.oublier() },
                )
            }
        }
    }
}

@Composable
private fun PastilleDeLaLiaison(l: EtatDeLaLiaison) {
    val (couleur, mot) = when (l) {
        EtatDeLaLiaison.Branchee -> Erp.Succes to "en ligne"
        EtatDeLaLiaison.EnCours -> Erp.Avertissement to "liaison…"
        EtatDeLaLiaison.Refusee -> Color(0xFFF87171) to "refusé"
        EtatDeLaLiaison.Hors -> Color(0xFF94A3B8) to "hors ligne"
    }
    Row(
        Modifier.background(Color(0x22FFFFFF), RoundedCornerShape(10.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).background(couleur, CircleShape))
        Spacer(Modifier.size(5.dp))
        Text(mot, color = Color.White, fontSize = 11.sp)
    }
}

/** Le visage d'Eve (son portrait, sinon son orbe), l'anneau de ce qu'elle fait, et ce qu'elle en dit. */
@Composable
private fun EtatDEve(m: Modele) {
    val eve = m.eve
    val parle by m.voix.parle.collectAsState()
    val actif = eve.etat in setOf("ecoute", "transcription", "reflexion", "parole", "confirmation") || m.ecoute || parle
    // L'anneau ne bat que quand Eve écoute, réfléchit ou parle : au repos, rien ne se redessine (la batterie).
    val battement = if (actif) {
        val vie = rememberInfiniteTransition(label = "anneau")
        val b by vie.animateFloat(0f, 1f, infiniteRepeatable(tween(1400), RepeatMode.Reverse), label = "bat")
        b
    } else {
        0f
    }
    val couleur = when {
        m.ecoute || eve.etat == "ecoute" -> Erp.Succes
        eve.etat == "confirmation" -> Erp.Avertissement
        eve.etat == "erreur" -> Erp.DangerEncre
        else -> Erp.Ciel
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(56.dp), contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val epaisseur = if (actif) 2.5f + 2f * battement else 2f
                drawCircle(couleur.copy(alpha = if (actif) 0.55f + 0.45f * battement else 0.5f), style = Stroke(epaisseur))
            }
            val portrait = m.portrait
            if (portrait != null) {
                Image(
                    portrait, contentDescription = "Eve", contentScale = ContentScale.Crop,
                    modifier = Modifier.size(48.dp).clip(CircleShape).background(Color(0xFFE8F0FE)),
                )
            } else {
                Orbe(Modifier.size(48.dp), vivante = actif)
            }
        }
        Spacer(Modifier.size(12.dp))
        Column(Modifier.weight(1f)) {
            // La sourdine de l'ordinateur est celle de ses haut-parleurs : le téléphone, lui, entend Eve (sauf la sienne).
            val libelle = when {
                m.ecoute -> "Je t'écoute…"
                m.liaison != EtatDeLaLiaison.Branchee -> "Je ne joins pas l'ordinateur"
                parle -> "Je réponds"
                eve.etat == "pret" -> if (m.sourdine) "Prête · en sourdine ici" else "Prête"
                else -> eve.libelle.ifBlank { "Prête" }
            }
            Text(libelle, color = Erp.Titre, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            val aide = when {
                m.ecoute -> "Relâche le micro quand tu as fini"
                eve.etat == "pret" -> "Tiens le micro pour me parler, ou écris-moi"
                else -> ""
            }
            if (aide.isNotEmpty()) Text(aide, color = Erp.Texte2, fontSize = 13.sp)
        }
        if (eve.occupee) {
            OutlinedButton(onClick = m::arreter, shape = RoundedCornerShape(10.dp)) {
                Icon(painterResource(R.drawable.ic_arreter), contentDescription = null, Modifier.size(14.dp))
                Spacer(Modifier.size(6.dp))
                Text("Arrêter")
            }
        }
    }
}

/** Une bulle : celles d'Eve à gauche, blanches au liseré bleu ; celles de la personne à droite, en dégradé bleu ; les
 * mots d'Eve elle-même (un jumelage, une panne) au milieu, en petit. */
@Composable
private fun Bulle(l: Ligne, enCours: Boolean = false) {
    when (l.de) {
        "toi" -> Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            Column(
                Modifier
                    .widthIn(max = 300.dp)
                    .background(Brush.linearGradient(Erp.Toi), RoundedCornerShape(14.dp, 14.dp, 4.dp, 14.dp))
                    .padding(horizontal = 12.dp, vertical = 9.dp)
            ) {
                Text(l.texte, color = Color.White, fontSize = 15.sp)
                if (l.pieces.isNotEmpty()) {
                    Text("📎 " + l.pieces.joinToString(", "), color = Color(0xDDFFFFFF), fontSize = 12.sp)
                }
            }
        }
        "eve" -> Row(Modifier.fillMaxWidth()) {
            Column(
                Modifier
                    .widthIn(max = 320.dp)
                    .background(
                        Brush.linearGradient(listOf(Color.White, Color(0xFFF8FAFC), Color(0xFFF0F4FF))),
                        RoundedCornerShape(4.dp, 14.dp, 14.dp, 14.dp),
                    )
                    .border(1.dp, Erp.FiletDeCarte, RoundedCornerShape(4.dp, 14.dp, 14.dp, 14.dp))
                    .padding(start = 12.dp, end = 12.dp, top = 9.dp, bottom = 9.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                for (morceau in morceaux(l.texte, Erp.Azur)) {
                    when (morceau) {
                        is Morceau.Texte -> Text(
                            morceau.texte,
                            color = if (morceau.detail) Erp.Texte2 else Erp.Ardoise,
                            fontSize = if (morceau.detail) 13.sp else 15.sp,
                        )
                        is Morceau.Code -> Text(
                            morceau.texte, fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = Color(0xFFE6EDF3),
                            modifier = Modifier.fillMaxWidth().background(Color(0xFF0D1117), RoundedCornerShape(8.dp))
                                .padding(8.dp),
                        )
                        is Morceau.Carte -> Text(
                            "Ici, ${morceau.sorte} : il s'affiche à l'ordinateur.",
                            color = Erp.Azur, fontSize = 13.sp, fontStyle = FontStyle.Italic,
                            modifier = Modifier.background(Erp.Fond2, RoundedCornerShape(8.dp))
                                .padding(horizontal = 8.dp, vertical = 5.dp),
                        )
                    }
                }
                if (enCours) Text("…", color = Erp.Texte3)
                if (l.tour.isNotBlank()) Text(l.tour, color = Erp.Texte3, fontSize = 11.sp)
            }
        }
        else -> Text(
            l.texte,
            color = if (l.de == "alerte") Color(0xFF92400E) else Erp.Texte2,
            fontSize = 12.sp,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp),
        )
    }
}

@Composable
private fun CarteDAccord(quoi: String, detail: String, confirmer: () -> Unit, annuler: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .background(Color(0xFFFFFBEB), RoundedCornerShape(14.dp))
            .border(1.dp, Color(0xFFFCD34D), RoundedCornerShape(14.dp))
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("J'attends ton accord", color = Color(0xFF92400E), fontWeight = FontWeight.SemiBold)
        Text(quoi, color = Erp.Titre, fontSize = 15.sp)
        if (detail.isNotBlank()) Text(detail, color = Erp.Texte2, fontSize = 13.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Button(
                onClick = confirmer,
                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF0B6E0B)),
                shape = RoundedCornerShape(10.dp),
            ) { Text("Je confirme") }
            OutlinedButton(onClick = annuler, shape = RoundedCornerShape(10.dp)) { Text("Annuler") }
        }
    }
}

@Composable
private fun Message(texte: String, fermer: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .background(Color(0xFFFFFBEB), RoundedCornerShape(10.dp))
            .padding(start = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(texte, color = Color(0xFF92400E), fontSize = 13.sp, modifier = Modifier.weight(1f).padding(vertical = 8.dp))
        IconButton(onClick = fermer) {
            Icon(painterResource(R.drawable.ic_fermer), contentDescription = "Fermer", tint = Color(0xFF92400E))
        }
    }
}

@Composable
private fun PiecesJointes(m: Modele) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        for (p in m.eve.pieces) {
            Row(
                Modifier.background(Color.White, RoundedCornerShape(8.dp)).border(1.dp, Erp.FiletDeCarte, RoundedCornerShape(8.dp))
                    .padding(start = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(p.nom, fontSize = 12.sp, color = Erp.Texte, maxLines = 1, modifier = Modifier.widthIn(max = 160.dp))
                IconButton(onClick = { m.retirerPiece(p) }, modifier = Modifier.size(28.dp)) {
                    Icon(painterResource(R.drawable.ic_fermer), contentDescription = "Retirer", Modifier.size(14.dp))
                }
            }
        }
    }
}

/** La saisie : le trombone (une photo), le champ, et le micro à tenir — ou l'envoi, quand le champ a du texte. */
@Composable
private fun Saisie(m: Modele, choisirUnePhoto: () -> Unit) {
    var texte by rememberSaveable { mutableStateOf("") }
    Row(
        Modifier
            .fillMaxWidth()
            .background(Color(0xF2FFFFFF))
            .navigationBarsPadding()
            .padding(horizontal = 8.dp, vertical = 8.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        IconButton(onClick = choisirUnePhoto) {
            Icon(painterResource(R.drawable.ic_joindre), contentDescription = "Joindre une photo", tint = Erp.Texte2)
        }
        OutlinedTextField(
            value = texte,
            onValueChange = { texte = it },
            placeholder = { Text("Écris à Eve…") },
            maxLines = 5,
            shape = RoundedCornerShape(14.dp),
            colors = OutlinedTextFieldDefaults.colors(
                focusedContainerColor = Color.White, unfocusedContainerColor = Color.White,
                unfocusedBorderColor = Erp.FiletDeCarte,
            ),
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.size(6.dp))
        if (texte.isNotBlank()) {
            IconButton(
                onClick = { m.ecrire(texte); texte = "" },
                modifier = Modifier.size(52.dp).background(Erp.Primaire, CircleShape),
            ) {
                Icon(painterResource(R.drawable.ic_envoyer), contentDescription = "Envoyer", tint = Color.White)
            }
        } else {
            BoutonDuMicro(m)
        }
    }
}

/** Le micro, tenu pour parler (comme la touche de l'ordinateur) : il écoute tant que le doigt reste, son onde suit la
 * voix ; relâché, ce qui a été dit part. */
@Composable
private fun BoutonDuMicro(m: Modele) {
    // Le niveau de la voix, relu à chaque image tant que le micro écoute ; sinon, rien ne bouge.
    var niveau by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(m.ecoute) {
        while (m.ecoute) withFrameMillis { niveau = m.niveauDuMicro }
        niveau = 0f
    }
    Box(
        Modifier
            .size(52.dp)
            .background(if (m.ecoute) Erp.Succes else Erp.Primaire, CircleShape)
            .pointerInput(Unit) {
                detectTapGestures(onPress = {
                    if (m.parlerDebut()) {
                        tryAwaitRelease()
                        m.parlerFin()
                    }
                })
            },
        contentAlignment = Alignment.Center,
    ) {
        if (m.ecoute) {
            // L'onde : un disque pâle qui suit le niveau de la voix.
            Canvas(Modifier.fillMaxSize()) {
                drawCircle(Color.White.copy(alpha = 0.35f), radius = size.minDimension / 2 * (0.62f + 0.38f * niveau),
                    center = Offset(size.width / 2, size.height / 2))
            }
        }
        Icon(painterResource(R.drawable.ic_micro), contentDescription = "Tenir pour parler", tint = Color.White)
    }
}

/** L'écran d'un jumelage en cours : le téléphone se présente à l'ordinateur. */
@Composable
fun JumelageEnCours() {
    FondQuiRespire {
        Column(Modifier.fillMaxSize()) {
            Barre(sous = null)
            Column(
                Modifier.fillMaxSize(),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Orbe(Modifier.size(140.dp))
                Spacer(Modifier.height(16.dp))
                Text("Je me présente à l'ordinateur…", color = Erp.Titre, fontSize = 17.sp)
            }
        }
    }
}
