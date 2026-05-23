package com.user.mycelime.data.entity

import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "expeditions")
data class Expedition(
    @PrimaryKey val id: String,
    val creatureId: String,
    val locationId: String,
    val startTime: Long,
    val durationMs: Long,
    val isReturned: Boolean = false,
    val bytesEarned: Long = 0L,
    val resultText: String = "",
)
