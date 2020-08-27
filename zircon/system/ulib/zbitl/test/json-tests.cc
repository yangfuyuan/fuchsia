// Copyright 2020 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <lib/zbitl/view.h>

#include <rapidjson/prettywriter.h>
#include <rapidjson/stringbuffer.h>

#include "src/lib/files/scoped_temp_dir.h"
#include "tests.h"

namespace {

using JsonBuffer = rapidjson::GenericStringBuffer<rapidjson::UTF8<char>>;

constexpr char kEmptyZbiJson[] = R"""({
  "offset": 0,
  "type": "CONTAINER",
  "size": 0,
  "items": []
})""";

constexpr char kSimpleZbiJson[] = R"""({
  "offset": 0,
  "type": "CONTAINER",
  "size": 48,
  "items": [
    {
      "offset": 32,
      "type": "IMAGE_ARGS",
      "size": 11,
      "crc32": 3608077223
    }
  ]
})""";

TEST(ZbitlViewJsonTests, EmptyZbi) {
  files::ScopedTempDir dir;
  fbl::unique_fd fd;
  size_t size = 0;
  ASSERT_NO_FATAL_FAILURES(OpenTestDataZbi(TestDataZbiType::kEmpty, dir.path(), &fd, &size));

  char buff[kMaxZbiSize];
  ASSERT_EQ(size, read(fd.get(), buff, size));

  zbitl::View view(std::string_view{buff, size});

  JsonBuffer buffer;
  rapidjson::PrettyWriter<JsonBuffer> json_writer(buffer);
  json_writer.SetIndent(' ', 2);
  JsonWriteZbi(json_writer, view, 0);
  EXPECT_STR_EQ(kEmptyZbiJson, buffer.GetString());

  auto error = view.take_error();
  EXPECT_FALSE(error.is_error(), "%s at offset %#x",
               std::string(error.error_value().zbi_error).c_str(),  // No '\0'.
               error.error_value().item_offset);
}

TEST(ZbitlViewJsonTests, SimpleZbi) {
  files::ScopedTempDir dir;
  fbl::unique_fd fd;
  size_t size = 0;
  ASSERT_NO_FATAL_FAILURES(OpenTestDataZbi(TestDataZbiType::kOneItem, dir.path(), &fd, &size));

  char buff[kMaxZbiSize];
  ASSERT_EQ(size, read(fd.get(), buff, size));

  zbitl::View view(std::string_view{buff, size});

  JsonBuffer buffer;
  rapidjson::PrettyWriter<JsonBuffer> json_writer(buffer);
  json_writer.SetIndent(' ', 2);
  JsonWriteZbi(json_writer, view, 0);
  EXPECT_STR_EQ(kSimpleZbiJson, buffer.GetString());

  auto error = view.take_error();
  EXPECT_FALSE(error.is_error(), "%s at offset %#x",
               std::string(error.error_value().zbi_error).c_str(),  // No '\0'.
               error.error_value().item_offset);
}

}  // namespace
