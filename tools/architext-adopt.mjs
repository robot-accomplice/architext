#!/usr/bin/env node
import { main } from "../src/adapters/cli/architext-cli.mjs";

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
