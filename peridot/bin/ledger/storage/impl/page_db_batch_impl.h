// Copyright 2017 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef PERIDOT_BIN_LEDGER_STORAGE_IMPL_PAGE_DB_BATCH_IMPL_H_
#define PERIDOT_BIN_LEDGER_STORAGE_IMPL_PAGE_DB_BATCH_IMPL_H_

#include "peridot/bin/ledger/coroutine/coroutine.h"
#include "peridot/bin/ledger/storage/impl/page_db.h"
#include "peridot/bin/ledger/storage/public/db.h"

namespace storage {

class PageDbBatchImpl : public PageDb::Batch {
 public:
  explicit PageDbBatchImpl(std::unique_ptr<Db::Batch> batch, PageDb* db);
  ~PageDbBatchImpl() override;

  // Heads.
  Status AddHead(coroutine::CoroutineHandler* handler, CommitIdView head,
                 zx::time_utc timestamp) override;
  Status RemoveHead(coroutine::CoroutineHandler* handler,
                    CommitIdView head) override;

  // Merges.
  Status AddMerge(coroutine::CoroutineHandler* handler, CommitIdView parent1_id,
                  CommitIdView parent2_id,
                  CommitIdView merge_commit_id) override;
  // Commits.
  Status AddCommitStorageBytes(coroutine::CoroutineHandler* handler,
                               const CommitId& commit_id,
                               fxl::StringView storage_bytes) override;
  Status RemoveCommit(coroutine::CoroutineHandler* handler,
                      const CommitId& commit_id) override;

  // Object data.
  Status WriteObject(coroutine::CoroutineHandler* handler,
                     ObjectIdentifier object_identifier,
                     std::unique_ptr<DataSource::DataChunk> content,
                     PageDbObjectStatus object_status) override;
  Status SetObjectStatus(coroutine::CoroutineHandler* handler,
                         ObjectIdentifier object_identifier,
                         PageDbObjectStatus object_status) override;

  // Commit sync metadata.
  Status MarkCommitIdSynced(coroutine::CoroutineHandler* handler,
                            const CommitId& commit_id) override;
  Status MarkCommitIdUnsynced(coroutine::CoroutineHandler* handler,
                              const CommitId& commit_id,
                              uint64_t generation) override;

  // Object sync metadata.
  Status SetSyncMetadata(coroutine::CoroutineHandler* handler,
                         fxl::StringView key, fxl::StringView value) override;

  // Page online state.
  Status MarkPageOnline(coroutine::CoroutineHandler* handler) override;

  Status Execute(coroutine::CoroutineHandler* handler) override;

 private:
  Status DCheckHasObject(coroutine::CoroutineHandler* handler,
                         const ObjectIdentifier& key);

  std::unique_ptr<Db::Batch> batch_;
  PageDb* const db_;

  FXL_DISALLOW_COPY_AND_ASSIGN(PageDbBatchImpl);
};

}  // namespace storage

#endif  // PERIDOT_BIN_LEDGER_STORAGE_IMPL_PAGE_DB_BATCH_IMPL_H_
