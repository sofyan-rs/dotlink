package com.dotlink.link

import java.util.UUID

/**
 * Files sent together (`batch`, `count`, `total` from the sender): one Accept or Decline
 * answers all of them.
 *
 * @param key the sender and its batch id, so another device can't use the answer.
 * @param total all the files' bytes, -1 when unknown.
 */
class IncomingBatch(
    val key: String,
    val count: Int,
    val total: Long,
) {
    companion object {
        /** From what [device] sent; null for a single file or an older sender. */
        fun from(
            device: String,
            batch: String?,
            count: Int,
            total: Long,
        ): IncomingBatch? = if (batch.isNullOrEmpty() || count <= 1) null else IncomingBatch("$device/$batch", count, total)

        /** The upload query for sending files of these [sizes] together ("" for one). */
        fun query(sizes: List<Long>): String {
            if (sizes.size <= 1) return ""
            // Left out when a size is unknown: the PC takes only a number there.
            val total = if (sizes.any { it < 0 }) "" else "&total=${sizes.sum()}"
            return "&batch=${UUID.randomUUID()}&count=${sizes.size}$total"
        }
    }
}
