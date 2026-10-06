//! The corpus model: the world builder's checks, clients, new inputs,
//! in-memory sources.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_corpus::delta::new_inputs;
use a2a_bench_corpus::source::{InMemory, TraceSource};
use a2a_bench_corpus::world::{
    CorpusError, Credential, StopReason, World, WorldAgent, WorldBuilder, credential_digest,
};
use a2a_bench_format::check::InputError;
use a2a_bench_format::exchange::{
    Client, Driven, Exchange, Fidelity, Request, Response, WorldDecl,
};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{ExchangeId, SourceRef, exchange_id};
use a2a_bench_format::labels::{ExchangeAgent, Label, Tier};
use a2a_bench_format::message::id_of;
use common::{agent_key, calls, dataset, draft, ok, result, says, system, tick, user, world_key};

fn builder() -> (WorldBuilder, WorldAgent, WorldAgent) {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let alice = ok(world.model_agent("alice", "gemini/flash"));
    let bob = ok(world.scripted_agent("bob"));
    (world, alice, bob)
}

#[test]
fn exchanges_carry_their_messages_time_and_client() {
    let (mut world, alice, _) = builder();
    let request = vec![system("sys"), user("hello")];
    let id = ok(world.exchange(draft(&alice, 3, request.clone(), says("hi there"))));
    let world = ok(world.finish(Coverage::Complete {
        tier: Tier::Construction,
    }));
    let exchange = world
        .exchange(id)
        .unwrap_or_else(|| panic!("exchange missing"));
    assert_eq!(exchange.at_us, tick(3));
    assert_eq!(
        exchange.request.messages,
        request.iter().map(|m| m.id()).collect::<Vec<_>>()
    );
    assert_eq!(exchange.response.messages, vec![says("hi there").id()]);
    assert_eq!(exchange.response.stop.as_deref(), Some("end_turn"));
    assert_eq!(world.agent_of(id), Some(alice.key()));
    assert_eq!(
        exchange.client,
        Client {
            credential: Credential::synthetic(&dataset(), &world_key("w"), &agent_key("alice"))
                .into_string(),
            session: None,
            turn: None,
            vendor: Some("google".into()),
            model: Some("gemini/flash".into()),
        }
    );
    // The id derives from dataset, source and time, as the format says.
    assert_eq!(
        id,
        exchange_id(
            &dataset(),
            &SourceRef::new("fixture.json", "/alice/3"),
            tick(3)
        )
    );
}

#[test]
fn drafts_carry_session_turn_model_and_tools() {
    let (mut world, alice, _) = builder();
    let mut d = draft(&alice, 1, vec![user("q")], says("r"));
    d.session = Some("conv-1".into());
    d.turn = Some(4);
    d.model = Some("gemini/pro".into());
    d.stop = Some(StopReason::ToolUse);
    d.tools = Some(Vec::new());
    let id = ok(world.exchange(d));
    let world = ok(world.finish(Coverage::Partial));
    let exchange = world.exchange(id).unwrap_or_else(|| panic!("missing"));
    assert_eq!(exchange.client.session.as_deref(), Some("conv-1"));
    assert_eq!(exchange.client.turn, Some(4));
    assert_eq!(exchange.client.model.as_deref(), Some("gemini/pro"));
    assert_eq!(exchange.response.stop.as_deref(), Some("tool_use"));
    assert_eq!(exchange.request.tools, Some(Vec::new()));
}

#[test]
fn recorded_ids_and_credentials_are_kept() {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let shared = Credential::recorded("k:group-1");
    let a = ok(world.model_agent_with("a", "m", shared.clone()));
    let b = ok(world.model_agent_with("b", "m", shared.clone()));
    let recorded = ExchangeId::from_raw(42);
    assert_eq!(
        world.recorded_exchange(recorded, draft(&a, 1, vec![user("q")], says("r"))),
        Ok(recorded)
    );
    assert!(matches!(
        world.recorded_exchange(recorded, draft(&b, 2, vec![user("q")], says("r"))),
        Err(CorpusError::DuplicateExchange(id)) if id == recorded
    ));
    let world = ok(world.finish(Coverage::Partial));
    assert_eq!(world.exchanges()[0].client.credential, "k:group-1");
}

#[test]
fn repeated_messages_are_stored_once() {
    let (mut world, alice, _) = builder();
    let request = vec![user("same"), user("same"), user("other")];
    let id = ok(world.exchange(draft(&alice, 1, request, says("ok"))));
    ok(world.exchange(draft(&alice, 2, vec![user("same")], says("ok"))));
    let world = ok(world.finish(Coverage::Partial));
    let exchange = world.exchange(id).unwrap_or_else(|| panic!("missing"));
    assert_eq!(exchange.request.messages.len(), 3);
    assert_eq!(world.inputs().messages().count(), 3);
    // First-use order: same, other, ok.
    let order: Vec<_> = world.messages_in_order().iter().map(|m| m.id()).collect();
    assert_eq!(
        order,
        vec![user("same").id(), user("other").id(), says("ok").id()]
    );
}

#[test]
fn messages_hash_their_body() {
    let message = user("x");
    assert_eq!(id_of(message.body()), Ok(message.id()));
}

#[test]
fn scripted_unknown_and_foreign_agents_make_no_exchanges() {
    let (mut world, _, bob) = builder();
    assert!(matches!(
        world.exchange(draft(&bob, 1, vec![], says("x"))),
        Err(CorpusError::ScriptedAgent(_))
    ));
    let carol = WorldAgent::new(world_key("w"), agent_key("carol"));
    assert!(matches!(
        world.exchange(draft(&carol, 1, vec![], says("x"))),
        Err(CorpusError::UnknownAgent(_))
    ));
    let foreign = WorldAgent::new(world_key("other"), agent_key("alice"));
    assert!(matches!(
        world.exchange(draft(&foreign, 1, vec![], says("x"))),
        Err(CorpusError::ForeignAgent { .. })
    ));
    assert!(matches!(
        world.model_agent("bob", "m"),
        Err(CorpusError::DuplicateAgent(_))
    ));
    assert!(matches!(world.scripted_agent(""), Err(CorpusError::Key(_))));
}

#[test]
fn an_agents_exchanges_move_forward_in_time() {
    let (mut world, alice, _) = builder();
    assert!(
        world
            .exchange(draft(&alice, 5, vec![user("a")], says("b")))
            .is_ok()
    );
    assert!(matches!(
        world.exchange(draft(&alice, 5, vec![user("c")], says("d"))),
        Err(CorpusError::OutOfOrder { .. })
    ));
    assert!(matches!(
        world.exchange(draft(&alice, 4, vec![user("c")], says("d"))),
        Err(CorpusError::OutOfOrder { .. })
    ));
}

#[test]
fn duplicate_exchange_ids_are_refused() {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let a = ok(world.model_agent("a", "m"));
    let b = ok(world.model_agent("b", "m"));
    let mut first = draft(&a, 1, vec![user("q")], says("r"));
    first.source = SourceRef::new("f", "/same");
    let mut second = draft(&b, 1, vec![user("q")], says("r"));
    second.source = SourceRef::new("f", "/same");
    assert!(world.exchange(first).is_ok());
    assert!(matches!(
        world.exchange(second),
        Err(CorpusError::DuplicateExchange(_))
    ));
}

#[test]
fn finished_worlds_order_exchanges_by_time() {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let a = ok(world.model_agent("a", "m"));
    let b = ok(world.model_agent("b", "m"));
    for (agent, at) in [(&a, 9), (&b, 2), (&a, 12), (&b, 7)] {
        assert!(
            world
                .exchange(draft(agent, at, vec![user("q")], says("r")))
                .is_ok()
        );
    }
    let world = ok(world.finish(Coverage::Partial));
    let times: Vec<_> = world.exchanges().iter().map(|e| e.at_us).collect();
    assert_eq!(times, vec![tick(2), tick(7), tick(9), tick(12)]);
    // One exchange_agent row per exchange, in exchange order, first.
    let rows: Vec<_> = world
        .labels()
        .iter()
        .map(|label| match label {
            Label::ExchangeAgent(row) => (row.exchange, row.agent.as_str().to_owned()),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    let expected: Vec<_> = world
        .exchanges()
        .iter()
        .zip(["b", "b", "a", "a"])
        .map(|(e, agent)| (e.id, agent.to_owned()))
        .collect();
    assert_eq!(rows, expected);
    // Agents are declared in key order with their kind and model.
    let decl = world.decl();
    assert_eq!(decl.agents.len(), 2);
    assert!(decl.agents.iter().all(|a| a.driven == Driven::Model));
}

#[test]
fn exchange_agent_rows_are_the_builders() {
    let (mut world, alice, _) = builder();
    let id = ok(world.exchange(draft(&alice, 1, vec![user("q")], says("r"))));
    assert_eq!(
        world.label(Label::ExchangeAgent(ExchangeAgent {
            exchange: id,
            agent: alice.key().clone(),
        })),
        Err(CorpusError::ExchangeAgentLabel)
    );
}

#[test]
fn world_new_runs_the_format_checks() {
    let hello = user("hello");
    let exchange = Exchange {
        id: ExchangeId::from_raw(1),
        at_us: tick(1),
        client: Client {
            credential: "k:x".into(),
            session: None,
            turn: None,
            vendor: None,
            model: None,
        },
        request: Request {
            messages: vec![hello.id(), user("absent").id()],
            tools: None,
        },
        response: Response {
            messages: vec![],
            stop: None,
            error: None,
        },
        fidelity: Fidelity::Reconstructed,
        source: SourceRef::new("f", "/x"),
    };
    let decl = WorldDecl {
        key: world_key("w"),
        agents: vec![],
    };
    let world = World::new(
        dataset(),
        decl,
        vec![hello],
        vec![exchange],
        vec![],
        Coverage::Partial,
    );
    assert!(matches!(
        world,
        Err(CorpusError::Inputs {
            source: InputError::MissingMessage { .. },
            ..
        })
    ));
}

#[test]
fn credentials_are_stable_per_agent_and_distinct() {
    let d = dataset();
    let (w1, w2) = (world_key("w1"), world_key("w2"));
    let (alice, bob) = (agent_key("alice"), agent_key("bob"));
    let a1 = Credential::synthetic(&d, &w1, &alice);
    assert_eq!(a1, Credential::synthetic(&d, &w1, &alice));
    assert_ne!(a1, Credential::synthetic(&d, &w1, &bob));
    assert_ne!(a1, Credential::synthetic(&d, &w2, &alice));
    assert_ne!(
        a1,
        Credential::synthetic(
            &ok(a2a_bench_format::ids::DatasetId::new("other")),
            &w1,
            &alice
        )
    );
    assert_eq!(
        a1.as_str(),
        format!("k:{}", credential_digest(&d, &w1, &alice).to_hex())
    );
    // Field boundaries are unambiguous: ("ab", "c") is not ("a", "bc").
    assert_ne!(
        credential_digest(&d, &world_key("ab"), &agent_key("c")),
        credential_digest(&d, &world_key("a"), &agent_key("bc"))
    );
}

#[test]
fn new_inputs_are_the_multiset_difference() {
    let (mut world, alice, _) = builder();
    let first = vec![system("s"), user("go"), user("round")];
    let second = vec![
        system("s"),
        user("go"),
        user("round"),
        calls("c1", "send", r#"{"x":1}"#),
        result("c1", "sent"),
        user("round"),
        user("peer said hi"),
    ];
    let a = world.exchange(draft(&alice, 1, first, calls("c1", "send", r#"{"x":1}"#)));
    let b = world.exchange(draft(&alice, 2, second, says("done")));
    let (Ok(a), Ok(b)) = (a, b) else {
        panic!("exchanges")
    };
    let world = ok(world.finish(Coverage::Partial));
    let (Some(a), Some(b)) = (world.exchange(a), world.exchange(b)) else {
        panic!("missing")
    };
    let first_new: Vec<usize> = new_inputs(None, a).into_iter().map(|(at, _)| at).collect();
    assert_eq!(first_new, vec![0, 1, 2]);
    // The echoed call is the previous response; the second "round" is new.
    let second_new: Vec<usize> = new_inputs(Some(a), b)
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(second_new, vec![4, 5, 6]);
}

#[test]
fn truncated_histories_do_not_count_kept_messages_as_new() {
    let (mut world, alice, _) = builder();
    let long = vec![system("s"), user("old 1"), user("old 2"), user("recent")];
    let truncated = vec![system("s"), user("old 2"), user("recent"), user("fresh")];
    let a = world.exchange(draft(&alice, 1, long, says("r1")));
    let b = world.exchange(draft(&alice, 2, truncated, says("r2")));
    let (Ok(a), Ok(b)) = (a, b) else {
        panic!("exchanges")
    };
    let world = ok(world.finish(Coverage::Partial));
    let (Some(a), Some(b)) = (world.exchange(a), world.exchange(b)) else {
        panic!("missing")
    };
    let fresh: Vec<_> = new_inputs(Some(a), b);
    assert_eq!(fresh, vec![(3, user("fresh").id())]);
}

#[test]
fn in_memory_sources_stream_their_worlds() {
    let (world, _, _) = builder();
    let mut source = InMemory::new(dataset(), vec![ok(world.finish(Coverage::Partial))]);
    assert_eq!(source.dataset(), &dataset());
    assert_eq!(source.worlds().count(), 1);
    assert_eq!(source.worlds().count(), 0);
}

#[test]
fn recorded_clients_are_kept_as_they_are() {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let a = ok(world.model_agent("a", "openai/gpt-4o"));
    let recorded = Client {
        credential: "k:recorded".into(),
        session: Some("s-9".into()),
        turn: Some(3),
        vendor: None,
        model: Some("as-sent".into()),
    };
    let mut d = draft(&a, 1, vec![user("q")], says("r"));
    // The draft's model, session and turn give way to the recorded client.
    d.model = Some("ignored".into());
    d.session = Some("ignored".into());
    let id = ExchangeId::from_raw(7);
    assert_eq!(
        world.recorded_exchange_with_client(id, recorded.clone(), d),
        Ok(id)
    );
    // The usual checks still hold.
    assert!(matches!(
        world.recorded_exchange_with_client(
            ExchangeId::from_raw(8),
            recorded.clone(),
            draft(&a, 1, vec![user("q")], says("r"))
        ),
        Err(CorpusError::OutOfOrder { .. })
    ));
    let world = ok(world.finish(Coverage::Partial));
    assert_eq!(world.exchanges()[0].client, recorded);
    assert_eq!(world.agent_of(id), Some(a.key()));
}

#[test]
fn scripted_agents_may_name_their_model() {
    let mut world = WorldBuilder::new(dataset(), world_key("w"));
    let a = ok(world.model_agent("a", "m"));
    let s = ok(world.scripted_agent_with_model("s", "configured/model"));
    ok(world.exchange(draft(&a, 1, vec![user("q")], says("r"))));
    assert!(matches!(
        world.exchange(draft(&s, 2, vec![user("q")], says("r"))),
        Err(CorpusError::ScriptedAgent(_))
    ));
    let world = ok(world.finish(Coverage::Partial));
    let decl = world
        .decl()
        .agents
        .iter()
        .find(|agent| agent.key == *s.key())
        .unwrap_or_else(|| panic!("s undeclared"));
    assert_eq!(decl.driven, Driven::Scripted);
    assert_eq!(decl.model.as_deref(), Some("configured/model"));
}

#[test]
fn notes_add_up_and_zero_is_not_recorded() {
    let (mut world, alice, _) = builder();
    ok(world.exchange(draft(&alice, 1, vec![user("q")], says("r"))));
    world.add_note("uncarried_control", 2);
    world.add_note("uncarried_control", 1);
    world.add_note("nothing", 0);
    let mut world = ok(world.finish(Coverage::Partial));
    world.add_note("key_group_not_a_cluster", 1);
    world.add_note("uncarried_control", 0);
    assert_eq!(
        world.notes(),
        &[
            ("key_group_not_a_cluster".to_owned(), 1),
            ("uncarried_control".to_owned(), 3),
        ]
        .into()
    );
}
