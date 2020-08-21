// Copyright 2020 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

package main

import (
	"flag"
	"log"
	"os"

	"go.fuchsia.dev/fuchsia/garnet/go/src/fidl/compiler/backend/types"
	"go.fuchsia.dev/fuchsia/tools/fidl/fidlgen_libfuzzer/codegen"
)

var jsonPath = flag.String("json", "",
	"relative path to the FIDL intermediate representation.")
var outputBase = flag.String("output-base", "",
	"the base file name for files generated by this generator.")
var includeBase = flag.String("include-base", "",
	"the directory to which C and C++ includes should be relative.")
var clangFormatPath = flag.String("clang-format-path", "",
	"path to the clang-format tool.")

func flagsValid() bool {
	return *jsonPath != "" && *outputBase != "" && *includeBase != ""
}

func main() {
	flag.Parse()
	if !flag.Parsed() || !flagsValid() {
		flag.PrintDefaults()
		os.Exit(1)
	}

	fidl, err := types.ReadJSONIr(*jsonPath)
	if err != nil {
		log.Fatal(err)
	}

	config := types.Config{
		OutputBase:  *outputBase,
		IncludeBase: *includeBase,
	}
	if err := codegen.NewFidlGenerator().GenerateFidl(fidl, &config, *clangFormatPath); err != nil {
		log.Fatalf("Error running generator: %v", err)
	}
}
