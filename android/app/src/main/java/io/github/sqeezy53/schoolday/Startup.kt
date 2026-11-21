package io.github.sqeezy53.schoolday

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.first

@Composable
internal fun StartupScene(ready: Boolean, content: @Composable () -> Unit) {
    val elapsed = remember { Animatable(0f) }
    val workspaceAlpha = remember { Animatable(0f) }
    var showWorkspace by remember { mutableStateOf(false) }
    val dataReady by rememberUpdatedState(ready)

    LaunchedEffect(Unit) {
        elapsed.animateTo(800f, tween(800, easing = LinearEasing))
        snapshotFlow { dataReady }.first { it }
        elapsed.animateTo(950f, tween(150, easing = LinearEasing))
        workspaceAlpha.snapTo(0.12f)
        showWorkspace = true
        workspaceAlpha.animateTo(1f, tween(150))
    }

    val startingColor = colorResource(R.color.school_day_launch_background)
    val surface = MaterialTheme.colorScheme.surface
    val background by remember(startingColor, surface) {
        derivedStateOf {
            if (startingColor == surface) surface
            else lerp(startingColor, surface, phase(elapsed.value, 0f, 180f))
        }
    }
    SubcomposeLayout(Modifier.fillMaxSize().drawBehind { drawRect(background) }) { constraints ->
        val workspace =
            if (showWorkspace || dataReady) {
                subcompose("workspace") {
                    Box(Modifier.fillMaxSize().graphicsLayer { alpha = workspaceAlpha.value }) {
                        content()
                    }
                }.single().measure(constraints)
            } else null
        // The workspace can be measured early, but only the active scene is placed.
        val active =
            if (showWorkspace) checkNotNull(workspace)
            else subcompose("startup") { StartupArtwork(elapsed) }.single().measure(constraints)

        layout(active.width, active.height) { active.placeRelative(0, 0) }
    }
}

@Composable
private fun StartupArtwork(elapsed: Animatable<Float, *>) {
    BoxWithConstraints(
        Modifier.fillMaxSize().safeDrawingPadding(),
        contentAlignment = Alignment.Center,
    ) {
        val logoSize = minOf(120.dp, maxHeight * 0.46f)
        Image(
            painterResource(R.drawable.school_day_launch_logo),
            contentDescription = null,
            modifier = Modifier.size(logoSize).graphicsLayer {
                val logo = phase(elapsed.value, 0f, 300f)
                val scale = 0.90f + 0.10f * logo
                scaleX = scale
                scaleY = scale
                alpha = logo * remaining(elapsed.value)
            },
        )
        Text(
            "Школьный день",
            modifier = Modifier.offset(y = logoSize / 2 + 26.dp).graphicsLayer {
                alpha = phase(elapsed.value, 250f, 300f) * remaining(elapsed.value)
            },
            color = MaterialTheme.colorScheme.onSurface,
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
        )
    }
}

private fun remaining(elapsed: Float): Float = 1f - ((elapsed - 800f) / 150f).coerceIn(0f, 1f)

private fun phase(elapsed: Float, start: Float, duration: Float): Float {
    val rest = 1f - ((elapsed - start) / duration).coerceIn(0f, 1f)
    return 1f - rest * rest
}
