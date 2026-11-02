// Retired local publisher (#16802); ambient credentials grant no authority.
const { createReporter } = require('./reporter');

createReporter('refuse-local-publication').error(
    'Direct local extension publication is disabled. Follow the reviewed release transaction (#13768 / #6055); see docs/release/RUNBOOK.md.',
);
process.exitCode = 1;
