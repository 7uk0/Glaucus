package com.user.mycelime.util

fun Long.toByteDisplay(): String = when {
    this >= 1_000_000_000_000L -> "%.2f TB".format(this / 1_000_000_000_000.0)
    this >= 1_000_000_000L     -> "%.2f GB".format(this / 1_000_000_000.0)
    this >= 1_000_000L         -> "%.2f MB".format(this / 1_000_000.0)
    this >= 1_000L             -> "%.2f KB".format(this / 1_000.0)
    else                       -> "$this Bytes"
}
