package ca.constructoai.eve.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/**
 * L'orbe d'Eve, en petit : un cœur de lumière bleue qui respire, sous une lueur et un reflet de verre. C'est l'idée de
 * celle de la fenêtre de l'ordinateur (`src\orbe.rs`, ses shaders), pas sa copie : le téléphone la montre tant qu'il ne
 * connaît pas encore le visage d'Eve. `vivante` : elle respire (chaque image se redessine) ; sinon, elle se tient au
 * milieu de son souffle, sans rien coûter.
 */
@Composable
fun Orbe(modifier: Modifier = Modifier, vivante: Boolean = true) {
    val souffle = if (vivante) {
        val vie = rememberInfiniteTransition(label = "orbe")
        // Le souffle de l'orbe : 4,5 s pour se gonfler, autant pour revenir, comme une respiration calme.
        val s by vie.animateFloat(
            initialValue = 0f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(4500, easing = FastOutSlowInEasing), RepeatMode.Reverse),
            label = "souffle",
        )
        s
    } else {
        0.5f
    }
    Canvas(modifier) {
        val r = size.minDimension / 2
        val c = center
        drawCircle(
            Brush.radialGradient(listOf(Color(0x6660A5FA), Color(0x2260A5FA), Color.Transparent), c, r),
            radius = r,
            center = c,
        )
        val coeur = r * (0.60f + 0.05f * souffle)
        drawCircle(
            Brush.radialGradient(
                0.0f to Color.White,
                0.25f to Color(0xFFBFDBFE),
                0.6f to Erp.Ciel,
                1.0f to Erp.Azur,
                center = Offset(c.x, c.y - coeur * 0.15f),
                radius = coeur * 1.1f,
            ),
            radius = coeur,
            center = c,
        )
        val reflet = Offset(c.x - coeur * 0.32f, c.y - coeur * 0.38f)
        drawCircle(
            Brush.radialGradient(listOf(Color(0x99FFFFFF), Color.Transparent), reflet, coeur * 0.45f),
            radius = coeur * 0.45f,
            center = reflet,
        )
    }
}
