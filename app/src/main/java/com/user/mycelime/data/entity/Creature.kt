package com.user.mycelime.data.entity

import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "creatures")
data class Creature(
    @PrimaryKey val id: String,
    val name: String,
    val lineageId: String,
    val power: Int,
    val resilience: Int,
    val speed: Int,
    val affinity: Int,
    val evolutionStage: Int = 0,
    val isAlive: Boolean = true,
    val dnaStored: Boolean = false,
)
