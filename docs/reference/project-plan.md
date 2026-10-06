# Project Plan
## Gerneral
- Task planning: [ota-outlaws-hack](https://github.com/orgs/Eclipse-SDV-Hackathon-Chapter-Four/projects/1)
- Communication: [ota-outlaws](https://app.slack.com/client/T02MS1M89UH/C0C6LDHN079)

## OTA Outlaws at a Glance

|Name|Git Handle|Responsibility|
|---|---|---|
|Sopan|[z0183379](https://github.com/z0183379)|CAN and uC|
|Youssef|[ya7903s](https://github.com/ya7903s)|KUKSA|
|Anukul|[anukul](https://github.com/anukul)|Battery Thermal Guardian and AutoSD|
|Jannis|[Jexpert19](https://github.com/Jexpert19)|Battery Thermal Guardian|
|Julian|[Bommelmann](https://github.com/Bommelmann)|KUKSA and uProtocol|
|Jens|[jens25](https://github.com/jens25)|AutoSD and Organizational|

## The Challenge
We decided to go for [Doctor Whodunit](https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Doctor-Whodunit) challenge.

## Our Solution
- Utilize `docker compose` to pull in `OpenSOVD Server`, `KUKSA CAN Provider` and `KUKSA Data Broker`
- Figure out a way to inject VSS description into `KUKSA Data Broker`
- Use rust as a robust and efficient way to implement our solution
    - Except of `Evidence Collector` and `Scenario Generator`, which will be implemented in Python
    - And `TempSensor` implementation, which will be implemented on the pcb and will be done in C.
- Outline faults and how to react to them (fault catalog)
- Implement a scenario generator
    - Provides fault CAN messages
    - Creates manifest, containing, Injection time (when the fault will happen), Fault ID
    - Evidence collector takes manifest into account and generates report
- Decouple the Battery Thermal Guardian from the Evidence Collector via OpenSOVD Server
- [Architecture specification](architecture.md)

## How We Work
- Task tracking and planning is done in a GitHub project: [ota-outlaws-hack](https://github.com/orgs/Eclipse-SDV-Hackathon-Chapter-Four/projects/1)
- Code quality is ensured by code review and by providing a development container, configured with pre commit hooks to build, test and lint code.
- Documents are hosted in git along code. See this folder
- Documentation follows the diataxis framework
- Team communication happens via a dedicated slack channel: [ota-outlaws](https://app.slack.com/client/T02MS1M89UH/C0C6LDHN079)
- Decision making is done via majority vote.