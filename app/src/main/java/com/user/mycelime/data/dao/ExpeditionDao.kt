package com.user.mycelime.data.dao

import androidx.room.*
import com.user.mycelime.data.entity.Expedition
import kotlinx.coroutines.flow.Flow

@Dao
interface ExpeditionDao {
    @Query("SELECT * FROM expeditions WHERE isReturned = 0")
    fun getActiveExpeditions(): Flow<List<Expedition>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(expedition: Expedition)

    @Update
    suspend fun update(expedition: Expedition)
}
