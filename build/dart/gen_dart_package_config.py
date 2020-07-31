#!/usr/bin/env python2.7
# Copyright 2020 The Fuchsia Authors. All rights reserved.
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
'''Reads the contents of a package config file generated by the build and
  converts it to a real package_config.json file
'''

import argparse
import collections
import json
import os
import re
import sys

DEFAULT_LANGUAGE_VERSION = "2.8"

Package = collections.namedtuple(
    'Package', ['name', 'rootUri', 'languageVersion', 'packageUri'])


class PackageConfig:
    # The version of the package config.
    VERSION = 2

    # The name of the generator which gets written to the json output
    GENERATOR_NAME = os.path.basename(__file__)

    def __init__(self, packages):
        self.packages = packages

    def asdict(self):
        '''Converts the package config to a dictionary'''
        return {
            'configVersion': self.VERSION,
            'packages': sorted(p._asdict() for p in self.packages),
            'generator': self.GENERATOR_NAME,
        }


def language_version_from_pubspec(pubspec):
    """ parse the content of a pubspec.yaml """
    import yaml
    with open(pubspec) as pubspec:
        parsed = yaml.safe_load(pubspec)
        if not parsed:
            return DEFAULT_LANGUAGE_VERSION

        # If a format like sdk: '>=a.b' or sdk: 'a.b' is found, we'll use a.b.
        # In all other cases we default to 2.8
        env_sdk = parsed.get('environment', {}).get('sdk', 'any')
        match = re.search(r"^(>=)?((0|[1-9]\d*)\.(0|[1-9]\d*))", env_sdk)
        if match:
            min_sdk_version = match.group(2)
        else:
            min_sdk_version = DEFAULT_LANGUAGE_VERSION

        return min_sdk_version


def collect_packages(items, relative_to):
    '''Reads metadata produced by GN and creates a list of packages.
  - items: a list of objects collected from gn
  - relative_to: The directory which the packages are relative to. This is
                  the location that contains the package_config.json file

  Returns None if there was a problem parsing packages
  '''
    packages = []
    for item in items:
        if 'language_version' in item:
            language_version = item['language_version']
        elif 'pubspec_path' in item:
            language_version = language_version_from_pubspec(
                item['pubspec_path'])
        else:
            language_version = DEFAULT_LANGUAGE_VERSION

        package = Package(
            name=item['name'],
            rootUri=os.path.relpath(item['root_uri'], relative_to),
            languageVersion=language_version,
            packageUri=item['package_uri'])

        #TODO(56428): enable once we sort out our duplicate packages
        # for p in packages:
        #   if p.rootUri == package.rootUri:
        #     print('Failed to create package_config.json file')
        #     print('The following packages contain the same package root ' + p.rootUri)
        #     print('  - ' + p.rootUri)
        #     print('  - ' + package.rootUri)
        #     return None

        packages.append(package)

    return packages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        '--input', help='Path to original package_config', required=True)
    parser.add_argument(
        '--output', help='Path to the updated package_config', required=True)
    parser.add_argument(
        '--root', help='Path to fuchsia root', required=True)
    args = parser.parse_args()

    sys.path += [os.path.join(args.root, 'third_party', 'pyyaml', 'lib')]

    with open(args.input, 'r') as input_file:
        contents = json.load(input_file)

    output_dir = os.path.dirname(os.path.abspath(args.output))
    packages = collect_packages(contents, output_dir)
    if packages is None:
        return 1

    with open(args.output, 'w') as output_file:
        package_config = PackageConfig(packages)
        json.dump(
            package_config.asdict(),
            output_file,
            indent=2,
            sort_keys=True,
            separators=(',', ': '))

    return 0


if __name__ == '__main__':
    sys.exit(main())
