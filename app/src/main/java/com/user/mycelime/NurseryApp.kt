package com.user.mycelime

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector

enum class InnerScreen {
    HABITAT, MAP, EVOLUTION, COMMAND, NURSERY
}

data class TabItem(
    val screen: InnerScreen,
    val label: String,
    val icon: ImageVector,
)

val tabs = listOf(
    TabItem(InnerScreen.HABITAT,   "Habitat",   Icons.Default.Home),
    TabItem(InnerScreen.MAP,       "Map",       Icons.Default.Place),
    TabItem(InnerScreen.EVOLUTION, "Evolution", Icons.Default.AccountTree),
    TabItem(InnerScreen.COMMAND,   "Command",   Icons.Default.Settings),
    TabItem(InnerScreen.NURSERY,   "Nursery",   Icons.Default.Egg),
)

@Composable
fun NurseryApp() {
    var currentScreen by remember { mutableStateOf(InnerScreen.HABITAT) }

    Scaffold(
        bottomBar = {
            NavigationBar {
                tabs.forEach { tab ->
                    NavigationBarItem(
                        selected = currentScreen == tab.screen,
                        onClick = { currentScreen = tab.screen },
                        icon = { Icon(tab.icon, contentDescription = tab.label) },
                        label = { Text(tab.label) }
                    )
                }
            }
        }
    ) { innerPadding ->
        Box(modifier = Modifier.padding(innerPadding).fillMaxSize()) {
            when (currentScreen) {
                InnerScreen.HABITAT   -> HabitatScreen()
                InnerScreen.MAP       -> MapScreen()
                InnerScreen.EVOLUTION -> EvolutionScreen()
                InnerScreen.COMMAND   -> CommandScreen()
                InnerScreen.NURSERY   -> NurseryScreen()
            }
        }
    }
}