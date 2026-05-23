package com.user.mycelime.data.dao

import androidx.room.*
import com.user.mycelime.data.entity.Egg
import kotlinx.coroutines.flow.Flow

@Dao
interface EggDao {
    @Query("SELECT * FROM eggs WHERE isHatched = 0")
    fun getUnhatchedEggs(): Flow<List<Egg>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(egg: Egg)

    @Update
    suspend fun update(egg: Egg)
}
