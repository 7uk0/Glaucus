package com.user.mycelime.data.entity

import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "eggs")
data class Egg(
    @PrimaryKey val id: String,
    val lineageId: String,
    val incubationStartTime: Long,
    val incubationDurationMs: Long,
    val isHatched: Boolean = false,
)
