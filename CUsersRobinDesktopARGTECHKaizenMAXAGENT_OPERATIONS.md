## Manager Operational Protocol
- I (Manager) own the 30-min heartbeat, unblocking issues, and answering agent questions. I do not freeze execution unless absolutely necessary.
- **Frontend Agent**: Assigned to build Leptos UI functionality continuously.
- **Review Agent**: Assigned to review UI/UX, feature completeness, and minimalistic aesthetics in real-time.
- **Test Agent**: Assigned to run cargo/trunk builds, test API reachability, and ensure no API overloads.
- Execution continues automatically based on task handoffs. If an agent fails, Manager immediately patches or re-tasks without waiting for user input.
