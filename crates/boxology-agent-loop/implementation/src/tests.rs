use super::*;
#[rustfmt::skip]
use boxology_contract::{BoxId, CallContext, Caller, CancelToken, CapabilityId, ErasedCallError, ErasedCallTarget, ExposureLevel, SlotValue, TraceContext};
#[rustfmt::skip]
use boxology_runtime::{AssemblyError, Composition, CompositionBuilder, ImportTarget, RemoteImportTarget, TransportExposure, test_support::StubTransport};
#[rustfmt::skip]
use std::{future::Future, pin::{Pin,pin}, sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}}, task::{Context,Poll,Waker}};
use tokio::sync::Notify;

#[rustfmt::skip]
fn context() -> CallContext { CallContext::new(Caller::Anonymous,None,CancelToken::new(),TraceContext::empty(),None) }
#[rustfmt::skip]
fn ready<F:Future>(future:F)->F::Output { let mut future=pin!(future); match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) { Poll::Ready(v)=>v, Poll::Pending=>panic!("pending") } }
#[rustfmt::skip]
struct Exposure(Vec<TransportExposure>);
#[rustfmt::skip]
impl ErasedCallTarget for Exposure { fn call<'a>(&'a self,id:&'a CapabilityId,ctx:CallContext,input:SlotValue)->Pin<Box<dyn Future<Output=Result<SlotValue,ErasedCallError>>+Send+'a>> { self.0.iter().find(|v|v.descriptor().id()==id).unwrap().dispatch(ctx,input) } }
#[rustfmt::skip]
#[derive(Clone)] struct Remote<T>{inner:T, ids:Arc<Vec<CapabilityId>>}
#[rustfmt::skip]
impl<T:ErasedCallTarget+Send+Sync> ErasedCallTarget for Remote<T>{fn call<'a>(&'a self,id:&'a CapabilityId,ctx:CallContext,input:SlotValue)->Pin<Box<dyn Future<Output=Result<SlotValue,ErasedCallError>>+Send+'a>>{self.inner.call(id,ctx,input)}}
#[rustfmt::skip]
impl<T:ErasedCallTarget+Send+Sync> RemoteImportTarget for Remote<T>{fn supports_capability(&self,id:&CapabilityId)->bool{self.ids.contains(id)}}
#[rustfmt::skip]
fn remote<T:ErasedCallTarget+Send+Sync+'static>(inner:T, descriptor:&'static boxology_contract::ContractDescriptor)->ImportTarget { ImportTarget::remote(Arc::new(Remote{inner,ids:Arc::new(descriptor.capabilities().iter().map(|v|v.id().clone()).collect())})) }
#[rustfmt::skip]
fn request()->RunTurnRequest { RunTurnRequest { session_id:"session".into(),turn_id:"turn-1".into(),user_message:"read it".into(),system_prompt:"be exact".into(),tools:vec![ModelTool{name:"read".into(),description:"Read".into(),input_schema_json:r#"{"type":"object"}"#.into()}],max_output_tokens:Some(64) } }

#[rustfmt::skip]
fn assembled_with_hold(events:Arc<Mutex<Vec<session::SessionEvent>>>, trace:Arc<Mutex<Vec<String>>>, models:Arc<AtomicUsize>, tools:Arc<AtomicUsize>, ambiguous:bool,bad_load_at:usize,hold:Option<(Arc<Notify>,Arc<Notify>)>)->(Composition,boxology_generated_contract::AgentLoopHandle){
    use boxology_import_model_completion::test_support::ModelCompletionFake;
    use boxology_import_session_store::test_support::SessionStoreFake;
    use boxology_import_tool_runner::test_support::ToolRunnerFake;
    let session_events=events.clone(); let load_trace=trace.clone();let load_calls=Arc::new(AtomicUsize::new(0));
    let sessions=SessionStoreFake::new().with_load(move|_,_|{let events=session_events.lock().unwrap().clone();let load=load_calls.fetch_add(1,Ordering::SeqCst)+1;let hold=hold.clone();load_trace.lock().unwrap().push("load".into());async move{if load==1&&let Some((entered,release))=hold{entered.notify_one();release.notified().await}Ok(session::LoadOutcome{result:Some(session::LoadResult{next_sequence:events.len() as u64,events}),failure:(bad_load_at==load).then(||session::SessionFailure{class:session::SessionFailureClass::Local,code:"bad".into(),message:"bad".into(),retryable:false,side_effect_possible:false})})}})
        .with_append({let events=events.clone();let trace=trace.clone();move|_,request|{let mut events=events.lock().unwrap();trace.lock().unwrap().push(format!("append:{}",request.event.event_id));let sequence=request.expected_sequence;let appended=if let Some(old)=events.iter().find(|v|v.event_id==request.event.event_id){assert_eq!((old.sequence,old.payload_json.as_str()),(sequence,request.event.payload_json.as_str()));false}else{assert_eq!(sequence,events.len() as u64);events.push(session::SessionEvent{sequence,event_id:request.event.event_id,kind:request.event.kind,payload_json:request.event.payload_json});true};async move{if ambiguous&&appended{Ok(session::AppendOutcome{result:None,failure:Some(session::SessionFailure{class:session::SessionFailureClass::Local,code:"local_io".into(),message:"local io".into(),retryable:false,side_effect_possible:true})})}else{Ok(session::AppendOutcome{result:Some(session::AppendResult{sequence,appended}),failure:None})}}}});
    let model_trace=trace.clone();let model_calls=models.clone();let model=ModelCompletionFake::new().with_complete(move|_,request|{let n=model_calls.fetch_add(1,Ordering::SeqCst);model_trace.lock().unwrap().push(format!("model:{n}"));async move{if matches!(request.messages.last().unwrap().role,model::MessageRole::Tool){Ok(model::CompletionOutcome{completion:Some(model::CompletionResult{content:Some("done".into()),tool_calls:vec![],finish_reason:model::FinishReason::Stop,usage:model::TokenUsage{input_tokens:4,output_tokens:2,total_tokens:6}}),failure:None})}else{assert!(matches!(request.messages[0].role,model::MessageRole::System));assert!(matches!(request.messages.last().unwrap().role,model::MessageRole::User));Ok(model::CompletionOutcome{completion:Some(model::CompletionResult{content:None,tool_calls:vec![model::ToolCall{id:"call-1".into(),name:"read".into(),arguments_json:r#"{"path":"note.txt"}"#.into()}],finish_reason:model::FinishReason::ToolCalls,usage:model::TokenUsage{input_tokens:2,output_tokens:1,total_tokens:3}}),failure:None})}}});
    let tool_trace=trace.clone();let tool_calls=tools.clone();let tool=ToolRunnerFake::new().with_execute(move|_,request|{tool_calls.fetch_add(1,Ordering::SeqCst);tool_trace.lock().unwrap().push("tool".into());assert_eq!(request.read.unwrap().path,"note.txt");async move{Ok(tool::ExecuteOutcome{result:Some(tool::ExecuteResult{file:Some(tool::FileResult{operation:tool::FileOperation::Read,path:"note.txt".into(),content:Some("hello".into()),bytes:5,changed:false}),bash:None}),failure:None})}});
    let descriptor=generated::implementation_descriptor();let capability=descriptor.contract().capabilities()[0].id().clone();let transport=Arc::new(StubTransport::new());let mut builder=CompositionBuilder::new();
    builder.add_box(descriptor,|imports|{let deps=generated::typed_imports(&imports);generated::factory(AgentLoopService::new(deps.model_completion,deps.tool_runner,deps.session_store),imports)});
    let consumer=BoxId::new("agent-loop").unwrap();builder.resolve_import(consumer.clone(),BoxId::new("model-completion").unwrap(),remote(model,model::contract_descriptor()));builder.resolve_import(consumer.clone(),BoxId::new("tool-runner").unwrap(),remote(tool,tool::contract_descriptor()));builder.resolve_import(consumer.clone(),BoxId::new("session-store").unwrap(),remote(sessions,session::contract_descriptor()));builder.expose(consumer,capability,transport.clone(),ExposureLevel::CodeOnly);
    let composition=builder.start().unwrap();let handle=boxology_generated_contract::AgentLoopHandle::from_erased(Arc::new(Exposure(transport.runtime().unwrap().exposures().to_vec())));(composition,handle)
}

#[rustfmt::skip]
fn assembled(events:Arc<Mutex<Vec<session::SessionEvent>>>,trace:Arc<Mutex<Vec<String>>>,models:Arc<AtomicUsize>,tools:Arc<AtomicUsize>,ambiguous:bool,bad_load_at:usize)->(Composition,boxology_generated_contract::AgentLoopHandle){assembled_with_hold(events,trace,models,tools,ambiguous,bad_load_at,None)}

#[test]
#[rustfmt::skip]
fn mandatory_three_imports_fail_closed(){let mut builder=CompositionBuilder::new();builder.add_box(generated::implementation_descriptor(),|imports|{let deps=generated::typed_imports(&imports);generated::factory(AgentLoopService::new(deps.model_completion,deps.tool_runner,deps.session_store),imports)});let errors=builder.validate().unwrap_err();assert_eq!(errors.errors().len(),3);assert!(errors.errors().iter().all(|v|matches!(v,AssemblyError::MissingImportResolution{..})));}

#[test]
#[rustfmt::skip]
fn generated_handle_runs_exact_one_tool_trace_and_replays_complete_turn(){let events=Arc::new(Mutex::new(Vec::new()));let trace=Arc::new(Mutex::new(Vec::new()));let models=Arc::new(AtomicUsize::new(0));let tools=Arc::new(AtomicUsize::new(0));let (_composition,handle)=assembled(events.clone(),trace.clone(),models.clone(),tools.clone(),false,0);let result=ready(handle.run_turn(context(),request())).unwrap().result.unwrap();assert_eq!((result.answer.as_str(),result.tool_name.as_deref(),result.usage.total_tokens),("done",Some("read"),9));assert_eq!(*trace.lock().unwrap(),["load","append:turn-1.user","model:0","append:turn-1.call","tool","append:turn-1.result","model:1","append:turn-1.assistant"]);let saved=events.lock().unwrap().clone();assert_eq!(saved.iter().map(|v|v.sequence).collect::<Vec<_>>(),[0,1,2,3]);assert_eq!(serde_json::from_str::<Value>(&saved[0].payload_json).unwrap(),json!({"schema":"agent-loop-event@1","content":"read it"}));drop(handle);trace.lock().unwrap().clear();let (_fresh,replayed)=assembled(events,trace.clone(),models.clone(),tools.clone(),false,0);let replay=ready(replayed.run_turn(context(),request())).unwrap().result.unwrap();assert_eq!(replay.answer,"done");assert_eq!(*trace.lock().unwrap(),["load"]);assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst)),(2,1));}

#[rustfmt::skip]
fn event(sequence:u64,id:&str,kind:session::SessionEventKind,payload:Value)->session::SessionEvent{session::SessionEvent{sequence,event_id:id.into(),kind,payload_json:serde_json::to_string(&payload).unwrap()}}

#[test]
#[rustfmt::skip]
fn crash_prefixes_resume_only_after_recorded_tool_result(){let user=event(0,"turn-1.user",session::SessionEventKind::User,json!({"schema":"agent-loop-event@1","content":"read it"}));let call=event(1,"turn-1.call",session::SessionEventKind::ToolCall,json!({"schema":"agent-loop-event@1","tool_call_id":"call-1","name":"read","arguments_json":"{\"path\":\"note.txt\"}"}));let events=Arc::new(Mutex::new(vec![user,call]));let trace=Arc::new(Mutex::new(Vec::new()));let models=Arc::new(AtomicUsize::new(0));let tools=Arc::new(AtomicUsize::new(0));let (_c,handle)=assembled(events.clone(),trace.clone(),models.clone(),tools.clone(),false,0);let failure=ready(handle.run_turn(context(),request())).unwrap().failure.unwrap();assert_eq!(failure.code,"incomplete_tool_effect");assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst)),(0,0));events.lock().unwrap().push(event(2,"turn-1.result",session::SessionEventKind::ToolResult,json!({"schema":"agent-loop-event@1","tool_call_id":"call-1","name":"read","output_json":"{}"})));trace.lock().unwrap().clear();let (_fresh,resumed)=assembled(events.clone(),trace.clone(),models.clone(),tools.clone(),false,0);assert_eq!(ready(resumed.run_turn(context(),request())).unwrap().result.unwrap().answer,"done");assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst)),(1,0));assert_eq!(events.lock().unwrap().len(),4);}

#[test]
#[rustfmt::skip]
fn ambiguous_appends_reload_and_accept_exact_persisted_events(){let events=Arc::new(Mutex::new(Vec::new()));let trace=Arc::new(Mutex::new(Vec::new()));let (_c,handle)=assembled(events.clone(),trace.clone(),Arc::new(AtomicUsize::new(0)),Arc::new(AtomicUsize::new(0)),true,0);assert_eq!(ready(handle.run_turn(context(),request())).unwrap().result.unwrap().answer,"done");assert_eq!(events.lock().unwrap().len(),4);assert_eq!(trace.lock().unwrap().iter().filter(|v|v.as_str()=="load").count(),5);}

#[tokio::test]
#[rustfmt::skip]
async fn concurrent_same_turn_executes_tool_once_and_replays(){let events=Arc::new(Mutex::new(Vec::new()));let trace=Arc::new(Mutex::new(Vec::new()));let models=Arc::new(AtomicUsize::new(0));let tools=Arc::new(AtomicUsize::new(0));let entered=Arc::new(Notify::new());let release=Arc::new(Notify::new());let (_c,handle)=assembled_with_hold(events.clone(),trace.clone(),models.clone(),tools.clone(),false,0,Some((entered.clone(),release.clone())));let first=tokio::spawn({let handle=handle.clone();async move{handle.run_turn(context(),request()).await}});entered.notified().await;let second=tokio::spawn(async move{handle.run_turn(context(),request()).await});tokio::task::yield_now().await;assert_eq!(*trace.lock().unwrap(),["load"]);release.notify_one();assert_eq!(first.await.unwrap().unwrap().result.unwrap().answer,"done");assert_eq!(second.await.unwrap().unwrap().result.unwrap().answer,"done");assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst),events.lock().unwrap().len()),(2,1,4));}

#[tokio::test]
#[rustfmt::skip]
async fn queued_cancellation_returns_without_imports(){let trace=Arc::new(Mutex::new(Vec::new()));let entered=Arc::new(Notify::new());let release=Arc::new(Notify::new());let(_c,handle)=assembled_with_hold(Arc::new(Mutex::new(Vec::new())),trace.clone(),Arc::new(AtomicUsize::new(0)),Arc::new(AtomicUsize::new(0)),false,0,Some((entered.clone(),release.clone())));let first=tokio::spawn({let handle=handle.clone();async move{handle.run_turn(context(),request()).await}});entered.notified().await;let token=CancelToken::new();let queued_context=CallContext::new(Caller::Anonymous,None,token.clone(),TraceContext::empty(),None);let queued=tokio::spawn(async move{handle.run_turn(queued_context,request()).await});tokio::task::yield_now().await;assert_eq!(*trace.lock().unwrap(),["load"]);token.cancel();let failure=queued.await.unwrap().unwrap().failure.unwrap();assert!(matches!(failure.class,TurnFailureClass::Cancelled));assert_eq!(*trace.lock().unwrap(),["load"]);release.notify_one();assert!(first.await.unwrap().unwrap().result.is_some());}

#[test]
#[rustfmt::skip]
fn malformed_loads_are_internal_and_do_not_continue(){let malformed=Arc::new(Mutex::new(vec![event(1,"turn-1.user",session::SessionEventKind::User,json!({"schema":"agent-loop-event@1","content":"read it"}))]));for(events,bad_at)in[(malformed,0),(Arc::new(Mutex::new(Vec::new())),1)]{let trace=Arc::new(Mutex::new(Vec::new()));let models=Arc::new(AtomicUsize::new(0));let tools=Arc::new(AtomicUsize::new(0));let(_c,handle)=assembled(events,trace,models.clone(),tools.clone(),false,bad_at);assert!(matches!(ready(handle.run_turn(context(),request())),Err(boxology_contract::CallError::Domain(RunTurnError::Internal))));assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst)),(0,0));}}

#[test]
#[rustfmt::skip]
fn ambiguous_reload_result_and_failure_is_internal(){let models=Arc::new(AtomicUsize::new(0));let tools=Arc::new(AtomicUsize::new(0));let(_c,handle)=assembled(Arc::new(Mutex::new(Vec::new())),Arc::new(Mutex::new(Vec::new())),models.clone(),tools.clone(),true,2);assert!(matches!(ready(handle.run_turn(context(),request())),Err(boxology_contract::CallError::Domain(RunTurnError::Internal))));assert_eq!((models.load(Ordering::SeqCst),tools.load(Ordering::SeqCst)),(0,0));}

#[test]
#[rustfmt::skip]
fn oversized_schema_is_typed_input_before_imports(){let trace=Arc::new(Mutex::new(Vec::new()));let(_c,handle)=assembled(Arc::new(Mutex::new(Vec::new())),trace.clone(),Arc::new(AtomicUsize::new(0)),Arc::new(AtomicUsize::new(0)),false,0);let mut input=request();input.tools[0].input_schema_json=format!("{{\"x\":\"{}\"}}","a".repeat(TEXT));let failure=ready(handle.run_turn(context(),input)).unwrap().failure.unwrap();assert!(matches!(failure.class,TurnFailureClass::Input));assert!(trace.lock().unwrap().is_empty());}

#[test]
#[rustfmt::skip]
fn maximum_turn_id_produces_valid_event_ids(){let events=Arc::new(Mutex::new(Vec::new()));let(_c,handle)=assembled(events.clone(),Arc::new(Mutex::new(Vec::new())),Arc::new(AtomicUsize::new(0)),Arc::new(AtomicUsize::new(0)),false,0);let mut input=request();input.turn_id="a".repeat(64);assert!(ready(handle.run_turn(context(),input)).unwrap().result.is_some());assert!(events.lock().unwrap().iter().all(|event|event.event_id.len()<=128));}
