//! CONCEPT: end-to-end check of the two gRPC services over a real TCP socket on 127.0.0.1.
//! WHY: proves the generated tonic code works on this machine (O-03) before WP-3 and WP-4 build on it,
//!      and shows the intended call pattern: the coordinator calls AgentService.Execute; agents call
//!      CoordinatorService.Register then keep one StreamHeartbeats stream open.
//! DEPENDS ON: generated services in contracts::v1; tokio (dev-dependency) for the runtime.
//! AFFECTS: nothing at runtime. These toy servers are NOT the real agent/coordinator.

use contracts::v1::agent_service_client::AgentServiceClient;
use contracts::v1::agent_service_server::{AgentService, AgentServiceServer};
use contracts::v1::coordinator_service_client::CoordinatorServiceClient;
use contracts::v1::coordinator_service_server::{CoordinatorService, CoordinatorServiceServer};
use contracts::*;
use tonic::transport::server::TcpIncoming;
use tonic::{Request, Response, Status, Streaming};

struct ToyAgent;

#[tonic::async_trait]
impl AgentService for ToyAgent {
    async fn execute(&self, req: Request<Command>) -> Result<Response<CommandResult>, Status> {
        let cmd = req.into_inner();
        let data = match cmd.args {
            Some(command::Args::StopRpcServer(_)) => {
                Some(command_result::Data::StopRpcServer(StopRpcServerResult {
                    exit_code: Some(0),
                }))
            }
            _ => None,
        };
        Ok(Response::new(CommandResult {
            schema_version: SCHEMA_VERSION,
            command_id: cmd.command_id,
            ok: data.is_some(),
            error: None,
            data,
        }))
    }
}

struct ToyCoordinator;

#[tonic::async_trait]
impl CoordinatorService for ToyCoordinator {
    async fn register(
        &self,
        req: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let caps = req.into_inner().capabilities.unwrap_or_default();
        Ok(Response::new(RegisterResponse {
            schema_version: SCHEMA_VERSION,
            accepted: !caps.node_id.is_empty(),
            error: None,
            coordinator_version: "test".into(),
        }))
    }

    async fn stream_heartbeats(
        &self,
        req: Request<Streaming<Heartbeat>>,
    ) -> Result<Response<HeartbeatStreamSummary>, Status> {
        let mut stream = req.into_inner();
        let mut received = 0;
        while let Some(hb) = stream.message().await? {
            assert_eq!(hb.seq, received + 1, "heartbeats arrive in order");
            received += 1;
        }
        Ok(Response::new(HeartbeatStreamSummary {
            schema_version: SCHEMA_VERSION,
            received,
        }))
    }
}

#[tokio::test]
async fn agent_and_coordinator_services_over_tcp() {
    let incoming = TcpIncoming::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = incoming.local_addr().unwrap();
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(AgentServiceServer::new(ToyAgent))
            .add_service(CoordinatorServiceServer::new(ToyCoordinator))
            .serve_with_incoming(incoming),
    );
    let url = format!("http://{addr}");

    // coordinator -> agent
    let mut agent = AgentServiceClient::connect(url.clone()).await.unwrap();
    let res = agent
        .execute(Command {
            schema_version: SCHEMA_VERSION,
            command_id: "cmd-1".into(),
            args: Some(command::Args::StopRpcServer(StopRpcServerArgs {})),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(res.ok);
    assert_eq!(res.command_id, "cmd-1");
    assert!(matches!(
        res.data,
        Some(command_result::Data::StopRpcServer(StopRpcServerResult {
            exit_code: Some(0)
        }))
    ));

    // agent -> coordinator
    let mut coord = CoordinatorServiceClient::connect(url).await.unwrap();
    let reg = coord
        .register(RegisterRequest {
            schema_version: SCHEMA_VERSION,
            capabilities: Some(NodeCapabilities {
                node_id: "n1".into(),
                ..Default::default()
            }),
            agent_addr: "127.0.0.1:1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(reg.accepted);

    let beats = (1..=3).map(|seq| Heartbeat {
        schema_version: SCHEMA_VERSION,
        node_id: "n1".into(),
        seq,
        ..Default::default()
    });
    let summary = coord
        .stream_heartbeats(tokio_stream_iter(beats))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(summary.received, 3);
}

/// A finite stream from an iterator, without pulling in tokio-stream just for this test.
fn tokio_stream_iter<I: Iterator<Item = Heartbeat> + Send + 'static>(
    it: I,
) -> impl tonic::codegen::tokio_stream::Stream<Item = Heartbeat> + Send + 'static {
    tonic::codegen::tokio_stream::iter(it)
}
