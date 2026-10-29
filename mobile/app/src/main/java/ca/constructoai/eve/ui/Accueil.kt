package ca.constructoai.eve.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ca.constructoai.eve.R

/** Le fond de l'ERP : un dégradé à −45° du blanc aux bleus pâles, comme celui de la fenêtre de l'ordinateur
 * (`theme::fond`) — figé au milieu de sa course, comme le fige l'ordinateur quand Windows ne veut pas d'animations : sur
 * le téléphone, un fond qui respire redessinerait tout l'écran soixante fois par seconde, et viderait la batterie. */
@Composable
fun FondQuiRespire(contenu: @Composable () -> Unit) {
    Box(
        Modifier.fillMaxSize().drawBehind {
            val d = size.width + size.height
            val depart = Offset(-d * 0.75f, -d * 0.75f)
            drawRect(
                Brush.linearGradient(
                    listOf(Color.White, Erp.Fond1, Erp.Fond2, Color.White, Erp.Fond1),
                    start = depart,
                    end = Offset(depart.x + d * 2f, depart.y + d * 2f),
                )
            )
        }
    ) { contenu() }
}

/** La barre marine de l'ERP, en haut : le logo en blanc, « EVE de Constructo AI », puis ce qu'on lui donne dessous. */
@Composable
fun Barre(sous: String?) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(Brush.verticalGradient(listOf(Erp.MarineHaut, Erp.Marine)))
            .statusBarsPadding()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(painterResource(R.drawable.logo_blanc), contentDescription = "Constructo AI", Modifier.size(30.dp))
        Spacer(Modifier.size(10.dp))
        Column {
            Text(
                buildAnnotatedString {
                    withStyle(SpanStyle(fontWeight = FontWeight.Bold, fontSize = 18.sp, letterSpacing = 1.sp)) {
                        append("EVE")
                    }
                    withStyle(SpanStyle(fontSize = 14.sp)) { append(" de Constructo AI") }
                },
                color = Color.White,
            )
            if (sous != null) Text(sous, color = Color(0xCCFFFFFF), fontSize = 12.sp)
        }
    }
}

/** L'accueil d'un téléphone qui n'est pas encore jumelé : ce qu'il faut faire sur l'ordinateur, puis ici ; `message` :
 * pourquoi le jumelage d'avant n'a pas abouti. */
@Composable
fun Accueil(scanner: () -> Unit, message: String?) {
    FondQuiRespire {
        Column(Modifier.fillMaxSize()) {
            Barre(sous = null)
            Column(
                Modifier.fillMaxSize().navigationBarsPadding().padding(horizontal = 20.dp, vertical = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Spacer(Modifier.height(24.dp))
                    Orbe(Modifier.size(180.dp))
                    Spacer(Modifier.height(16.dp))
                    Text("Bonjour !", color = Erp.Titre, fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        "Pour me parler d'ici, jumelle ce téléphone avec moi, sur ton ordinateur.",
                        color = Erp.Texte2,
                        fontSize = 16.sp,
                    )
                }
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Card(
                        shape = RoundedCornerShape(14.dp),
                        colors = CardDefaults.cardColors(containerColor = Color.White),
                        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
                    ) {
                        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Etape(1, "Sur l'ordinateur, ouvre mes réglages, à la rubrique « Téléphones ».")
                            Etape(2, "Choisis « Jumeler un téléphone » : un code carré s'affiche.")
                            Etape(3, "Ici, touche « Scanner le code » et vise-le.")
                        }
                    }
                    if (message != null) {
                        Text(
                            message,
                            color = Color(0xFF92400E),
                            fontSize = 14.sp,
                            modifier = Modifier.fillMaxWidth()
                                .background(Color(0xFFFFFBEB), RoundedCornerShape(10.dp))
                                .padding(12.dp),
                        )
                    }
                }
                Button(
                    onClick = scanner,
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                    shape = RoundedCornerShape(12.dp),
                ) {
                    Text("Scanner le code", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

@Composable
private fun Etape(numero: Int, texte: String) {
    Row(verticalAlignment = Alignment.Top) {
        Box(
            Modifier.size(26.dp).background(Erp.Fond2, RoundedCornerShape(13.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Text("$numero", color = Erp.Azur, fontWeight = FontWeight.Bold, fontSize = 14.sp)
        }
        Spacer(Modifier.size(12.dp))
        Text(texte, color = Erp.Texte, fontSize = 15.sp, modifier = Modifier.padding(top = 2.dp))
    }
}
