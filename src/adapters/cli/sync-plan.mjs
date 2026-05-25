export function syncOperation({ installing, migrating }) {
  if (installing) return "install";
  if (migrating) return "migrate";
  return "sync";
}

export function syncWritePlan({ installing, migrating, doctorRepairAvailable, syncChoices, options }) {
  const doctorRepairsSelected = Boolean(doctorRepairAvailable && syncChoices.applyDoctorRepairs);
  const shouldWrite = Boolean(
    installing
    || migrating
    || doctorRepairsSelected
    || options.force
    || syncChoices.instructionFiles.length > 0
    || syncChoices.manageGitignore
    || syncChoices.manageRootScripts
  );
  const operation = syncOperation({ installing, migrating });
  return {
    doctorRepairsSelected,
    shouldWrite,
    operation,
    operationLabel: `Operation: ${operation}${shouldWrite ? "" : " (current)"}`
  };
}

export function shouldValidateSync({ options, installing }) {
  return !(options.skipValidate || (options.dryRun && installing));
}

export function persistedSyncChoices(choices) {
  return {
    branch: choices.branch,
    instructionFiles: choices.instructionFiles,
    manageGitignore: choices.manageGitignore,
    manageRootScripts: choices.manageRootScripts,
    applyDoctorRepairs: choices.applyDoctorRepairs,
    proceedWithChanges: choices.proceedWithChanges
  };
}

export function syncMetadataPatch({
  version,
  installing,
  migrating,
  instructionFiles,
  syncChoices,
  managedInstructions,
  gitignoreManaged,
  rootScriptsManaged,
  validation,
  now
}) {
  return {
    source: "architext-cli",
    cliVersion: version,
    operation: syncOperation({ installing, migrating }),
    dataPolicy: installing ? "starter-written" : "preserved",
    copiedInstallMigrated: migrating,
    instructionFiles: Object.fromEntries(instructionFiles.map((fileName) => [fileName, syncChoices.instructionFiles.includes(fileName)])),
    managedInstructions,
    gitignoreManaged,
    rootScriptsManaged,
    syncChoices: persistedSyncChoices(syncChoices),
    lastValidation: validation ? { ok: validation.ok, at: now } : undefined
  };
}
