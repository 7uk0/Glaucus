package com.user.mycelime

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import com.user.mycelime.viewmodel.NurseryViewModel

@Composable
fun HabitatScreen() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("HABITAT")
    }
}

@Composable
fun MapScreen() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("MAP")
    }
}

@Composable
fun EvolutionScreen() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("EVOLUTION")
    }
}

@Composable
fun CommandScreen() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("COMMAND")
    }
}

@Composable
fun NurseryScreen(
    viewModel: NurseryViewModel = viewModel(
        factory = NurseryViewModel.factory(
            LocalContext.current.applicationContext as MYcelimeApp
        )
    )
) {
    val eggs by viewModel.eggs.collectAsState()
    val bytesDisplay by viewModel.bytesDisplay.collectAsState()

    Column(
        modifier = Modifier.fillMaxSize(),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Eggs incubating: ${eggs.size}")
        Text("Balance: $bytesDisplay")
    }
}
