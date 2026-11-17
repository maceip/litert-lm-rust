#!/usr/bin/env node
// Copyright 2026 LiteRT-LM Rust Contributors
// SPDX-License-Identifier: Apache-2.0

import crypto from "node:crypto";
import fs from "node:fs";

const [tag, version, target, commitSha, upstreamRef, repoSlug, purlArch, purlOs, fileList] =
  process.argv.slice(2);

if (!tag || !version || !target || !commitSha || !upstreamRef || !repoSlug || !purlArch || !purlOs || !fileList) {
  console.error(
    "Usage: node scripts/generate-sbom.mjs <tag> <version> <target> <commitSha> <upstreamRef> <repoSlug> <purlArch> <purlOs> <commaSeparatedFiles>"
  );
  process.exit(1);
}

const archive = `litert-lm-${tag}-${target}.tar.gz`;
const sha256 = (p) => crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");
const archiveSha = sha256(`dist/${archive}`);
const created = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
const baseUrl = `https://github.com/${repoSlug}/releases/download/${tag}`;
const files = fileList.split(",").filter(Boolean);

const packages = [
  {
    name: `litert-lm-${target}`,
    SPDXID: "SPDXRef-ReleaseArchive",
    versionInfo: version,
    downloadLocation: `${baseUrl}/${archive}`,
    filesAnalyzed: false,
    licenseConcluded: "Apache-2.0",
    licenseDeclared: "Apache-2.0",
    copyrightText: "Copyright The ODML Authors / Google LLC",
    checksums: [{ algorithm: "SHA256", checksumValue: archiveSha }],
    externalRefs: [
      {
        referenceCategory: "PACKAGE-MANAGER",
        referenceType: "purl",
        referenceLocator: `pkg:github/${repoSlug}@${tag}?arch=${purlArch}&os=${purlOs}`,
      },
    ],
  },
  {
    name: "google-ai-edge-litert-lm",
    SPDXID: "SPDXRef-UpstreamLiteRtLm",
    versionInfo: upstreamRef,
    downloadLocation: `git+https://github.com/google-ai-edge/LiteRT-LM.git@${upstreamRef}`,
    filesAnalyzed: false,
    licenseConcluded: "Apache-2.0",
    licenseDeclared: "Apache-2.0",
    copyrightText: "Copyright The ODML Authors / Google LLC",
  },
];

const relationships = [
  {
    spdxElementId: "SPDXRef-DOCUMENT",
    relationshipType: "DESCRIBES",
    relatedSpdxElement: "SPDXRef-ReleaseArchive",
  },
  {
    spdxElementId: "SPDXRef-ReleaseArchive",
    relationshipType: "GENERATED_FROM",
    relatedSpdxElement: "SPDXRef-UpstreamLiteRtLm",
  },
];

for (const file of files) {
  const id = `SPDXRef-${file.replace(/[^A-Za-z0-9.-]+/g, "-")}`;
  packages.push({
    name: file,
    SPDXID: id,
    versionInfo: version,
    downloadLocation: `${baseUrl}/${file}`,
    filesAnalyzed: false,
    licenseConcluded: "Apache-2.0",
    licenseDeclared: "Apache-2.0",
    copyrightText: "Copyright The ODML Authors / Google LLC",
    checksums: [{ algorithm: "SHA256", checksumValue: sha256(`dist/${file}`) }],
  });
  relationships.push({
    spdxElementId: "SPDXRef-ReleaseArchive",
    relationshipType: "CONTAINS",
    relatedSpdxElement: id,
  });
}

const sbom = {
  spdxVersion: "SPDX-2.3",
  dataLicense: "CC0-1.0",
  SPDXID: "SPDXRef-DOCUMENT",
  name: `litert-lm-${tag}-${target}`,
  documentNamespace: `${baseUrl}/${archive}.spdx.json/${archiveSha}`,
  creationInfo: {
    created,
    creators: [
      `Tool: litert-lm-rust-release-${version}`,
      `Organization: ${repoSlug.split("/")[0]}`,
    ],
    comment: `Built from ${repoSlug}@${commitSha} and google-ai-edge/LiteRT-LM@${upstreamRef} (${target})`,
  },
  packages,
  relationships,
};

fs.writeFileSync(`dist/litert-lm-${tag}-${target}.spdx.json`, JSON.stringify(sbom, null, 2) + "\n");
