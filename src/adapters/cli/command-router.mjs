export function createCommandHandlers(dependencies) {
  return {
    install: dependencies.sync,
    upgrade: dependencies.sync,
    sync: dependencies.sync,
    migrate: dependencies.sync,
    serve: dependencies.serve,
    validate: dependencies.validate,
    build: dependencies.build,
    prompt: dependencies.prompt,
    clean: dependencies.clean,
    explain: dependencies.explain,
    status: dependencies.status,
    doctor: dependencies.doctor
  };
}

export async function routeCommand({ options, target, handlers }) {
  const handler = handlers[options.command];
  if (!handler) throw new Error(`Unknown command: ${options.command}`);
  return handler(target, options);
}
