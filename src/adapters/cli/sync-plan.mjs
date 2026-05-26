export function normalizeSyncInstructionFiles(files, validInstructionFiles) {
  return validInstructionFiles.filter((fileName) => files?.includes(fileName));
}

export function defaultSyncChoices({ rootPackageExists, instructionFiles }) {
  return {
    branch: "current",
    instructionFiles,
    manageGitignore: true,
    manageRootScripts: rootPackageExists,
    applyDoctorRepairs: true,
    proceedWithChanges: true,
    promptBeforeProceed: false
  };
}

export function rememberedSyncChoices(metadata, { instructionFiles }) {
  const choices = metadata?.syncChoices;
  if (!choices || typeof choices !== "object") return null;
  return {
    branch: ["current", "new", "none"].includes(choices.branch) ? choices.branch : "current",
    instructionFiles: normalizeSyncInstructionFiles(choices.instructionFiles, instructionFiles),
    manageGitignore: Boolean(choices.manageGitignore),
    manageRootScripts: Boolean(choices.manageRootScripts),
    applyDoctorRepairs: choices.applyDoctorRepairs !== false,
    proceedWithChanges: choices.proceedWithChanges !== false,
    promptBeforeProceed: false
  };
}

export function applyExplicitSyncOptions(choices, options, { instructionFiles }) {
  const next = { ...choices };
  if (options.branch) next.branch = options.branch;
  if (options.noAgents) next.instructionFiles = [];
  else if (options.appendAgents) next.instructionFiles = instructionFiles;
  if (options.noGitignore) next.manageGitignore = false;
  else if (options.updateGitignore) next.manageGitignore = true;
  if (options.noRootScripts) next.manageRootScripts = false;
  else if (options.rootScripts) next.manageRootScripts = true;
  return next;
}

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
