# Impact map

The lifecycle blast radius includes runner install/register/run, systemd units, runner workspaces, workflow execution, and cache maintenance. The safe transition is fail-closed: keep services stopped, install immutable profile frontdoors, use volatile per-job state, and restart one slot only after policy and filesystem proofs pass.
