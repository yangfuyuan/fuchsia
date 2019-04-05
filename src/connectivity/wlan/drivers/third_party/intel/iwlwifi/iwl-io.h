/******************************************************************************
 *
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *  * Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *  * Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in
 *    the documentation and/or other materials provided with the
 *    distribution.
 *  * Neither the name Intel Corporation nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
 * A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
 * OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 *****************************************************************************/
#ifndef SRC_CONNECTIVITY_WLAN_DRIVERS_THIRD_PARTY_INTEL_IWLWIFI_IWL_IO_H_
#define SRC_CONNECTIVITY_WLAN_DRIVERS_THIRD_PARTY_INTEL_IWLWIFI_IWL_IO_H_

#include "iwl-devtrace.h"
#include "iwl-trans.h"

void iwl_write8(struct iwl_trans* trans, uint32_t ofs, uint8_t val);
void iwl_write32(struct iwl_trans* trans, uint32_t ofs, uint32_t val);
void iwl_write64(struct iwl_trans* trans, uint64_t ofs, uint64_t val);
uint32_t iwl_read32(struct iwl_trans* trans, uint32_t ofs);

static inline void iwl_set_bit(struct iwl_trans* trans, uint32_t reg, uint32_t mask) {
    iwl_trans_set_bits_mask(trans, reg, mask, mask);
}

static inline void iwl_clear_bit(struct iwl_trans* trans, uint32_t reg, uint32_t mask) {
    iwl_trans_set_bits_mask(trans, reg, mask, 0);
}

int iwl_poll_bit(struct iwl_trans* trans, uint32_t addr, uint32_t bits, uint32_t mask, int timeout);
int iwl_poll_direct_bit(struct iwl_trans* trans, uint32_t addr, uint32_t mask, int timeout);

uint32_t iwl_read_direct32(struct iwl_trans* trans, uint32_t reg);
void iwl_write_direct32(struct iwl_trans* trans, uint32_t reg, uint32_t value);
void iwl_write_direct64(struct iwl_trans* trans, uint64_t reg, uint64_t value);

uint32_t iwl_read_prph_no_grab(struct iwl_trans* trans, uint32_t ofs);
uint32_t iwl_read_prph(struct iwl_trans* trans, uint32_t ofs);
void iwl_write_prph_no_grab(struct iwl_trans* trans, uint32_t ofs, uint32_t val);
void iwl_write_prph64_no_grab(struct iwl_trans* trans, uint64_t ofs, uint64_t val);
void iwl_write_prph(struct iwl_trans* trans, uint32_t ofs, uint32_t val);
int iwl_poll_prph_bit(struct iwl_trans* trans, uint32_t addr, uint32_t bits, uint32_t mask,
                      int timeout);
void iwl_set_bits_prph(struct iwl_trans* trans, uint32_t ofs, uint32_t mask);
void iwl_set_bits_mask_prph(struct iwl_trans* trans, uint32_t ofs, uint32_t bits, uint32_t mask);
void iwl_clear_bits_prph(struct iwl_trans* trans, uint32_t ofs, uint32_t mask);
void iwl_force_nmi(struct iwl_trans* trans);

/* Error handling */
int iwl_dump_fh(struct iwl_trans* trans, char** buf);

#endif  // SRC_CONNECTIVITY_WLAN_DRIVERS_THIRD_PARTY_INTEL_IWLWIFI_IWL_IO_H_
