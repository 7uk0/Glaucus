package com.user.mycelime.data.entity

import androidx.room.Entity
import androidx.room.PrimaryKey

@Entity(tableName = "nursery_state")
data class NurseryState(
    @PrimaryKey val id: Int = 1,
    val bytesRaw: Long = 0L,
)
