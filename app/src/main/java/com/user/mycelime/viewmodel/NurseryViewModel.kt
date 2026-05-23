package com.user.mycelime.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.user.mycelime.MYcelimeApp
import com.user.mycelime.data.NurseryRepository
import com.user.mycelime.data.entity.Creature
import com.user.mycelime.data.entity.Egg
import com.user.mycelime.data.entity.Expedition
import com.user.mycelime.util.toByteDisplay
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn

class NurseryViewModel(private val repository: NurseryRepository) : ViewModel() {

    val eggs: StateFlow<List<Egg>> = repository.eggs
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val creatures: StateFlow<List<Creature>> = repository.creatures
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val activeExpeditions: StateFlow<List<Expedition>> = repository.activeExpeditions
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val bytesDisplay: StateFlow<String> = repository.nurseryState
        .map { it?.bytesRaw?.toByteDisplay() ?: "0 Bytes" }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), "0 Bytes")

    companion object {
        fun factory(app: MYcelimeApp) = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                NurseryViewModel(app.repository) as T
        }
    }
}
