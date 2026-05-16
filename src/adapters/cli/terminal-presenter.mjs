export function statusLines(status, { verbose = false } = {}) {
  const lines = [
    `Target: ${status.target}`,
    `Architext data: ${status.installed ? "installed" : "missing"}`,
    `CLI: ${status.cliVersion}`,
    `Copied install: ${status.copiedInstallDetected ? "detected" : "no"}`,
    `Gitignore: ${status.gitignoreMissing.length ? `missing ${status.gitignoreMissing.join(", ")}` : "ok"}`,
    `Generated artifacts tracked: ${status.trackedGenerated.length ? status.trackedGenerated.length : "none"}`
  ];

  if (status.c4) {
    lines.push(`C4 documents: ${status.c4.issues.length ? `${status.c4.issues.length} issue${status.c4.issues.length === 1 ? "" : "s"}` : "ok"}`);
  }
  if (status.releaseTruth) {
    lines.push(`Release Truth: ${status.releaseTruth.configured && status.releaseTruth.indexExists ? "configured" : status.releaseTruth.configured ? "index missing" : "not configured"}`);
  }

  lines.push(`Doctor repairs: ${status.doctorRepairs.length ? status.doctorRepairs.length : "none"}`);

  if (status.doctorRepairs.length && verbose) {
    lines.push("Doctor repairs available:");
    for (const repair of status.doctorRepairs) lines.push(`- ${repair.summary}`);
  }

  if (status.validation) {
    lines.push(`Validation: ${status.validation.ok ? "passed" : "failed"}`);
    if (!status.validation.ok || verbose) lines.push(status.validation.output);
  }

  if (verbose) {
    if (status.c4?.issues.length) {
      lines.push("C4 issues:");
      for (const issue of status.c4.issues) lines.push(`- ${issue}`);
    }
    if (status.c4?.remainingIssues.length) {
      lines.push("C4 issues requiring manual architecture judgment:");
      for (const issue of status.c4.remainingIssues) lines.push(`- ${issue}`);
    }

    lines.push("Instruction files:");
    for (const [fileName, fileStatus] of Object.entries(status.instructionStatus)) {
      const state = fileStatus.hasArchitextSection
        ? fileStatus.mentionsCopiedTemplate
          ? "outdated Architext section"
          : "current Architext section"
        : fileStatus.exists
          ? "missing Architext section"
          : "missing";
      lines.push(`- ${fileName}: ${state}`);
    }

    lines.push("Root scripts:");
    for (const [name, script] of Object.entries(status.rootScripts)) {
      lines.push(`- ${name}: ${script.present ? script.recommended ? "ok" : "custom" : "missing"}`);
    }
  }

  return lines;
}

export function printStatus(status, options) {
  for (const line of statusLines(status, options)) console.log(line);
}
