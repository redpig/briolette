package com.briolette.wallet.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val BriolettePrimary = Color(0xFF1B5E20)
private val BrioletteSecondary = Color(0xFF2E7D32)
private val BrioletteTertiary = Color(0xFF66BB6A)

private val DarkColorScheme = darkColorScheme(
    primary = BrioletteTertiary,
    secondary = BrioletteSecondary,
    tertiary = Color(0xFFA5D6A7),
    background = Color(0xFF121212),
    surface = Color(0xFF1E1E1E),
)

private val LightColorScheme = lightColorScheme(
    primary = BriolettePrimary,
    secondary = BrioletteSecondary,
    tertiary = BrioletteTertiary,
    background = Color(0xFFF5F5F5),
    surface = Color.White,
)

@Composable
fun BrioletteTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColorScheme else LightColorScheme,
        content = content,
    )
}
