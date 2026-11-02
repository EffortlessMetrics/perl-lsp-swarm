// Retired local publisher (#16802); ambient credentials grant no authority.
console.error(
  'Direct local extension publication is disabled. Follow the reviewed release transaction (#13768 / #6055); see docs/release/RUNBOOK.md.',
);
process.exitCode = 1;
