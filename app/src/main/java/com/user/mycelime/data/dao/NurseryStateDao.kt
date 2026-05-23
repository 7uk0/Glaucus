package com.user.mycelime.data.dao

import androidx.room.*
import com.user.mycelime.data.entity.NurseryState
import kotlinx.coroutines.flow.Flow

@Dao
interface NurseryStateDao {
    @Query("SELECT * FROM nursery_state WHERE id = 1")
    fun getNurseryState(): Flow<NurseryState?>

    @Upsert
    suspend fun upsert(state: NurseryState)
}
