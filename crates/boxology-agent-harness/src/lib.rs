#![forbid(unsafe_code)]
#![allow(clippy::possible_missing_else)]
use agent_loop_contract as contract;
use boxology_contract::{
    BoxId, CallContext, CallError, Caller, CancelToken, CapabilityDescriptor, CapabilityId,
    CapabilityShape, Deadline, Detail, ErasedCallError, ErasedCallTarget, ExposureLevel, SlotValue,
    TraceContext,
};
use boxology_runtime::{
    Composition, CompositionBuilder, ImportTarget, TransportBinding, TransportExposure,
    TransportHandle, TransportJoinFuture, TransportRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    future::Future,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
const SCHEMA: &str = "boxology.agent-harness@1";
const MAX_LINE: usize = 1024 * 1024;
const MAX_RECORDS: usize = 4096;

#[rustfmt::skip] #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Envelope{schema:String,id:String,method:String,params:Value,timeout_ms:Option<u64>}
#[rustfmt::skip] #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Turn{session_id:String,turn_id:String,user_message:String,system_prompt:String,tools:Vec<Tool>,max_output_tokens:Option<u64>}
#[rustfmt::skip] #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Tool{name:String,description:String,input_schema_json:String}
#[rustfmt::skip] #[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Compact{session_id:String,checkpoint_id:String,summary:String}
#[rustfmt::skip] #[derive(Serialize)] struct Response<'a>{schema:&'a str,id:Option<&'a str>,ok:bool,#[serde(skip_serializing_if="Option::is_none")]result:Option<Value>,#[serde(skip_serializing_if="Option::is_none")]error:Option<Failure>}
#[rustfmt::skip] #[derive(Serialize)] struct Failure{class:&'static str,code:String,message:String,retryable:bool,side_effect_possible:bool}
#[rustfmt::skip] fn fail(class:&'static str,code:&str)->Failure{Failure{class,code:code.into(),message:code.replace('_'," "),retryable:false,side_effect_possible:false}}
#[rustfmt::skip] fn internal(side_effect_possible:bool)->Failure{Failure{side_effect_possible,..fail("invocation","internal")}}
#[rustfmt::skip] fn valid_id(id:&str)->bool{!id.is_empty()&&id.len()<=128&&id.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b))}

/// Runs strict, bounded, sequential LF records through the generated agent-loop handle.
#[rustfmt::skip]
pub fn serve<R:BufRead,W:Write>(handle:&contract::AgentLoopHandle,input:R,output:W)->i32{serve_controlled(handle,input,output,Arc::new(Control::default()))}
#[derive(Default)]
struct Control {
    state: Mutex<ControlState>,
}
#[derive(Default)]
struct ControlState {
    stopped: bool,
    active: Option<CancelToken>,
}
impl Control {
    #[cfg(test)]
    fn stop(&self) -> bool {
        let mut state = self.state.lock().expect("control lock poisoned");
        state.stopped = true;
        let idle = state.active.is_none();
        if let Some(token) = state.active.as_ref() {
            token.cancel()
        }
        idle
    }
    fn signal_stop_or_exit(&self) {
        let mut state = self.state.lock().expect("control lock poisoned");
        state.stopped = true;
        if let Some(token) = state.active.as_ref() {
            token.cancel()
        } else {
            std::process::exit(0)
        }
    }
    fn stopped(&self) -> bool {
        self.state.lock().expect("control lock poisoned").stopped
    }
    fn begin(&self) -> CancelToken {
        let mut state = self.state.lock().expect("control lock poisoned");
        let token = CancelToken::new();
        if state.stopped {
            token.cancel()
        }
        state.active = Some(token.clone());
        token
    }
    fn end(&self) {
        self.state.lock().expect("control lock poisoned").active = None
    }
}
#[rustfmt::skip]
fn serve_controlled<R:BufRead,W:Write>(handle:&contract::AgentLoopHandle,mut input:R,mut output:W,control:Arc<Control>)->i32{let runtime=match tokio::runtime::Builder::new_current_thread().enable_time().build(){Ok(v)=>v,Err(_)=>return 1};let mut line=Vec::new();let mut ids=BTreeSet::new();let mut records=0;
 loop{if control.stopped(){return 0}let(read,lf,large)=match record(&mut input,&mut line){Ok(v)=>v,Err(error)if error.kind()==std::io::ErrorKind::Interrupted&&control.stopped()=>return 0,Err(_)=>return 1};if !read{return 0}records+=1;let guessed=||serde_json::from_slice::<Value>(&line).ok().and_then(|v|v.get("id").and_then(Value::as_str).filter(|id|valid_id(id)).map(str::to_owned));if records>MAX_RECORDS{let id=guessed();let _=emit(&mut output,id.as_deref(),None,Some(fail("protocol","request_limit")));return 1}if !lf{let _=emit(&mut output,None,None,Some(fail("protocol","unterminated_record")));return 1}if large{if emit(&mut output,None,None,Some(fail("protocol","record_too_large"))).is_err(){return 1}continue}if line.is_empty(){if emit(&mut output,None,None,Some(fail("protocol","blank_record"))).is_err(){return 1}continue}if line.last()==Some(&b'\r'){if emit(&mut output,None,None,Some(fail("protocol","invalid_record"))).is_err(){return 1}continue}
  let value:Value=match serde_json::from_slice(&line){Ok(v)=>v,Err(_)=>{if emit(&mut output,None,None,Some(fail("protocol","invalid_record"))).is_err(){return 1}continue}};let guessed=value.get("id").and_then(Value::as_str).filter(|v|valid_id(v)).map(str::to_owned);let req:Envelope=match serde_json::from_value(value){Ok(v)=>v,Err(_)=>{if emit(&mut output,guessed.as_deref(),None,Some(fail("protocol","invalid_request"))).is_err(){return 1}continue}};
  if req.schema!=SCHEMA||!valid_id(&req.id)||req.timeout_ms.is_some_and(|v|v==0||v>300_000){let id=valid_id(&req.id).then_some(req.id.as_str());if emit(&mut output,id,None,Some(fail("protocol","invalid_request"))).is_err(){return 1}continue}if !ids.insert(req.id.clone()){if emit(&mut output,Some(&req.id),None,Some(fail("protocol","duplicate_id"))).is_err(){return 1}continue}if !matches!(req.method.as_str(),"run_turn"|"compact"){if emit(&mut output,Some(&req.id),None,Some(fail("protocol","unknown_method"))).is_err(){return 1}continue}let token=control.begin();let(result,error)=match invoke(&runtime,handle,&req,token){Ok(v)=>(Some(v),None),Err(v)=>(None,Some(v))};let emitted=emit(&mut output,Some(&req.id),result,error);control.end();if emitted.is_err(){return 1}if control.stopped(){return 0}
 }}
#[rustfmt::skip]
fn record<R:BufRead>(input:&mut R,line:&mut Vec<u8>)->std::io::Result<(bool,bool,bool)>{line.clear();let mut read=false;let mut large=false;loop{let buf=input.fill_buf()?;if buf.is_empty(){return Ok((read,false,large))}read=true;let take=buf.iter().position(|v|*v==b'\n').map_or(buf.len(),|v|v+1);let content=if buf.get(take-1)==Some(&b'\n'){take-1}else{take};if !large&&line.len()+content<=MAX_LINE{line.extend_from_slice(&buf[..content])}else{large=true}let lf=buf.get(take-1)==Some(&b'\n');input.consume(take);if lf{return Ok((true,true,large))}}}
#[rustfmt::skip]
fn emit<W:Write>(out:&mut W,id:Option<&str>,result:Option<Value>,error:Option<Failure>)->std::io::Result<()>{let fallback_side=error.as_ref().is_none_or(|v|v.side_effect_possible);let bytes=serde_json::to_vec(&Response{schema:SCHEMA,id,ok:error.is_none(),result,error}).map_err(std::io::Error::other)?;let bytes=if bytes.len()>MAX_LINE{let mut failure=fail("protocol","response_too_large");failure.side_effect_possible=fallback_side;serde_json::to_vec(&Response{schema:SCHEMA,id,ok:false,result:None,error:Some(failure)}).map_err(std::io::Error::other)?}else{bytes};out.write_all(&bytes)?;out.write_all(b"\n")?;out.flush()}
#[rustfmt::skip]
fn invoke(rt:&tokio::runtime::Runtime,handle:&contract::AgentLoopHandle,req:&Envelope,token:CancelToken)->Result<Value,Failure>{let context=CallContext::new(Caller::Anonymous,req.timeout_ms.map(|v|Deadline::at(Instant::now()+Duration::from_millis(v))),token,TraceContext::empty(),None);match req.method.as_str(){"run_turn"=>{let v:Turn=serde_json::from_value(req.params.clone()).map_err(|_|fail("protocol","invalid_params"))?;let request=contract::RunTurnRequest{session_id:v.session_id,turn_id:v.turn_id,user_message:v.user_message,system_prompt:v.system_prompt,tools:v.tools.into_iter().map(|t|contract::ModelTool{name:t.name,description:t.description,input_schema_json:t.input_schema_json}).collect(),max_output_tokens:v.max_output_tokens};let out=rt.block_on(handle.run_turn(context,request)).map_err(call)?;project(out.result,out.failure,|r|json!({"answer":r.answer,"tool_name":r.tool_name,"tool_call_id":r.tool_call_id,"tool_output_json":r.tool_output_json,"usage":{"input_tokens":r.usage.input_tokens,"output_tokens":r.usage.output_tokens,"total_tokens":r.usage.total_tokens}}))},"compact"=>{let v:Compact=serde_json::from_value(req.params.clone()).map_err(|_|fail("protocol","invalid_params"))?;let out=rt.block_on(handle.compact(context,contract::CompactRequest{session_id:v.session_id,checkpoint_id:v.checkpoint_id,summary:v.summary})).map_err(call)?;project(out.result,out.failure,|r|json!({"event_id":r.event_id,"sequence":r.sequence,"appended":r.appended}))},_=>unreachable!()}}
#[rustfmt::skip]fn project<T>(result:Option<T>,failure:Option<contract::TurnFailure>,f:impl FnOnce(T)->Value)->Result<Value,Failure>{match(result,failure){(Some(v),None)=>Ok(f(v)),(None,Some(v))=>Err(domain(v)),_=>Err(internal(true))}}
#[rustfmt::skip]
fn domain(v:contract::TurnFailure)->Failure{let class=match v.class{contract::TurnFailureClass::Input=>"input",contract::TurnFailureClass::Session=>"session",contract::TurnFailureClass::Model=>"model",contract::TurnFailureClass::Tool=>"tool",contract::TurnFailureClass::Protocol=>"protocol",contract::TurnFailureClass::Cancelled=>"cancelled",contract::TurnFailureClass::Deadline=>"deadline",contract::TurnFailureClass::Unknown{..}=>return internal(v.side_effect_possible)};Failure{class,code:v.code,message:v.message,retryable:v.retryable,side_effect_possible:v.side_effect_possible}}
#[rustfmt::skip] fn call(v:CallError<contract::RunTurnError>)->Failure{let code=match v{CallError::Deadline=>"deadline",CallError::Cancelled=>"cancelled",_=>"internal"};Failure{side_effect_possible:true,..fail("invocation",code)}}

/// Owns the four local boxes and generated agent-loop handle.
pub struct Harness {
    _composition: Composition,
    handle: contract::AgentLoopHandle,
}
impl Harness {
    /// Assembles model, tool, session, and loop boxes with three local imports.
#[rustfmt::skip]
    pub fn start(root:PathBuf,state:PathBuf)->Result<Self,String>{let model=model_completion_implementation::XaiCompletionService::from_env().map_err(|_|"model configuration".to_owned())?;let tool=tool_runner_implementation::ToolRunnerService::new(root).map_err(|_|"tool configuration".to_owned())?;let sessions=session_store_implementation::SessionStoreService::new(state).map_err(|_|"session configuration".to_owned())?;let binding=Arc::new(LocalBinding::default());let mut b=CompositionBuilder::new();b.add_box(model_completion_implementation::generated::implementation_descriptor(),move|i|model_completion_implementation::generated::factory(model,i));b.add_box(tool_runner_implementation::generated::implementation_descriptor(),move|i|tool_runner_implementation::generated::factory(tool,i));b.add_box(session_store_implementation::generated::implementation_descriptor(),move|i|session_store_implementation::generated::factory(sessions,i));b.add_box(agent_loop_implementation::generated::implementation_descriptor(),|i|{let x=agent_loop_implementation::generated::typed_imports(&i);agent_loop_implementation::generated::factory(agent_loop_implementation::AgentLoopService::new(x.model_completion,x.tool_runner,x.session_store),i)});let id=BoxId::new("agent-loop").unwrap();for provider in["model-completion","tool-runner","session-store"]{let p=BoxId::new(provider).unwrap();b.resolve_import(id.clone(),p.clone(),ImportTarget::local(p));}for capability in agent_loop_implementation::generated::implementation_descriptor().contract().capabilities(){b.expose(id.clone(),capability.id().clone(),binding.clone(),ExposureLevel::CodeOnly);}let composition=b.start().map_err(|_|"composition startup".to_owned())?;let runtime=binding.runtime().ok_or("binding startup")?;if runtime.exposures().len()!=2{return Err("exposure startup".into())}let handle=contract::AgentLoopHandle::from_erased(Arc::new(Target(runtime.exposures().to_vec())));Ok(Self{_composition:composition,handle})}
}

/// Parses exact CLI paths, starts the composition, and serves stdin/stdout.
#[rustfmt::skip]
pub fn main_entry<R:BufRead,W:Write,E:Write>(args:Vec<OsString>,input:R,output:W,err:&mut E)->i32{let Some((root,state))=args2(&args)else{let _=writeln!(err,"invalid arguments");return 2};let h=match Harness::start(root,state){Ok(v)=>v,Err(_)=>{let _=writeln!(err,"startup failed");return 1}};let control=Arc::new(Control::default());if install_sigint(control.clone()).is_err(){let _=writeln!(err,"startup failed");return 1}serve_controlled(&h.handle,input,output,control)}
fn install_sigint(control: Arc<Control>) -> Result<(), std::io::Error> {
    let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGINT])?;
    std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            control.signal_stop_or_exit()
        }
    });
    Ok(())
}
#[rustfmt::skip] fn args2(args:&[OsString])->Option<(PathBuf,PathBuf)>{if args.len()!=4||args[0]!="--root"||args[2]!="--state-dir"{return None}Some((canonical(Path::new(&args[1]))?,canonical(Path::new(&args[3]))?))}
#[rustfmt::skip] fn canonical(p:&Path)->Option<PathBuf>{if !p.is_absolute()||std::fs::symlink_metadata(p).ok()?.file_type().is_symlink()||!p.is_dir(){return None}let c=std::fs::canonicalize(p).ok()?;(c==p).then_some(c)}

#[derive(Default)]
struct LocalBinding {
    runtime: Mutex<Option<Weak<TransportRuntime<()>>>>,
}
impl LocalBinding {
    fn runtime(&self) -> Option<Arc<TransportRuntime<()>>> {
        self.runtime.lock().ok()?.as_ref()?.upgrade()
    }
}
struct LocalHandle {
    _runtime: Arc<TransportRuntime<()>>,
}
impl TransportHandle for LocalHandle {
    fn stop_intake(&self) {}
    fn cancel_tasks(&self) {}
    fn abort_tasks(&self) {}
    fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
        Box::pin(std::future::ready(Ok(())))
    }
}
#[rustfmt::skip] impl TransportBinding for LocalBinding{type Config=();type Handle=LocalHandle;fn config(&self)->Arc<()>{Arc::new(())}fn conform(&self,d:&CapabilityDescriptor,_:ExposureLevel)->Result<(),Detail>{matches!(d.shape(),CapabilityShape::Unary).then_some(()).ok_or_else(||Detail::new("shape"))}fn prepare(&self,_:&[&'static CapabilityDescriptor])->Result<(),Detail>{Ok(())}fn start(&self,r:TransportRuntime<()>)->Result<LocalHandle,Detail>{let r=Arc::new(r);*self.runtime.lock().map_err(|_|Detail::new("lock"))?=Some(Arc::downgrade(&r));Ok(LocalHandle{_runtime:r})}}
struct Target(Vec<TransportExposure>);
#[rustfmt::skip] impl ErasedCallTarget for Target{fn call<'a>(&'a self,c:&'a CapabilityId,x:CallContext,v:SlotValue)->Pin<Box<dyn Future<Output=Result<SlotValue,ErasedCallError>>+Send+'a>>{match self.0.iter().find(|e|e.descriptor().id()==c){Some(e)=>e.dispatch(x,v),None=>Box::pin(std::future::ready(Err(ErasedCallError::Internal(Detail::new("capability")))))}}}

#[cfg(test)]#[rustfmt::skip]mod tests{use super::*;use contract::test_support::AgentLoopFake;use std::sync::atomic::Ordering;
fn fake()->contract::AgentLoopHandle{AgentLoopFake::new().with_run_turn(|context,_|async move{assert!(context.deadline().is_some());Ok(contract::RunTurnOutcome{result:Some(contract::RunTurnResult{answer:"ok".into(),tool_name:None,tool_call_id:None,tool_output_json:None,usage:contract::TurnUsage{input_tokens:1,output_tokens:2,total_tokens:3}}),failure:None})}).with_compact(|_,_|async{Ok(contract::CompactOutcome{result:Some(contract::CompactResult{event_id:"checkpoint.c".into(),sequence:7,appended:true}),failure:None})}).handle()}
fn request(id:&str)->String{format!(r#"{{"schema":"{SCHEMA}","id":"{id}","method":"run_turn","params":{{"session_id":"s","turn_id":"t","user_message":"u","system_prompt":"","tools":[],"max_output_tokens":null}},"timeout_ms":2}}"#)}
fn compact(id:&str)->String{format!(r#"{{"schema":"{SCHEMA}","id":"{id}","method":"compact","params":{{"session_id":"s","checkpoint_id":"c","summary":"summary"}}}}"#)}
fn run(s:&[u8])->(i32,String){let mut out=vec![];let code=serve(&fake(),s,&mut out);(code,String::from_utf8(out).unwrap())}
#[test]fn generated_handle_projects_correlated_result(){let(c,o)=run(format!("{}\n",request("a")).as_bytes());assert_eq!(c,0);let v:Value=serde_json::from_str(o.trim()).unwrap();assert_eq!((v["id"].as_str(),v["result"]["answer"].as_str()),(Some("a"),Some("ok")))}
#[test]fn framing_matrix(){let(c,o)=run(b"\n{}\r\nnot-json\n");assert_eq!(c,0);assert!(o.contains("blank_record")&&o.contains("invalid_record"));let(c,o)=run(b"{}");assert_eq!(c,1);assert!(o.contains("unterminated_record"));let mut input=std::io::Cursor::new([vec![b'a';MAX_LINE],b"\n".to_vec(),vec![b'b';MAX_LINE+1],b"\nnext\n".to_vec()].concat());let mut line=vec![];assert_eq!(record(&mut input,&mut line).unwrap(),(true,true,false));assert_eq!(line.len(),MAX_LINE);assert_eq!(record(&mut input,&mut line).unwrap(),(true,true,true));assert!(line.len()<=MAX_LINE);assert_eq!(record(&mut input,&mut line).unwrap(),(true,true,false));assert_eq!(line,b"next")}
#[test]fn output_limits_and_failure_truth(){let failure=contract::TurnFailure{class:contract::TurnFailureClass::Tool,code:"tool_failed".into(),message:"safe".into(),retryable:true,side_effect_possible:true};let h=AgentLoopFake::new().with_run_turn(move|_,_|{let f=failure.clone();async move{Ok(contract::RunTurnOutcome{result:None,failure:Some(f)})}}).handle();let mut out=vec![];assert_eq!(serve(&h,format!("{}\n",request("f")).as_bytes(),&mut out),0);let v:Value=serde_json::from_slice(&out).unwrap();assert_eq!(v["error"],json!({"class":"tool","code":"tool_failed","message":"safe","retryable":true,"side_effect_possible":true}));out.clear();emit(&mut out,Some("x"),Some(json!({"huge":"x".repeat(MAX_LINE)})),None).unwrap();let v:Value=serde_json::from_slice(&out).unwrap();assert_eq!((v["error"]["code"].as_str(),v["error"]["side_effect_possible"].as_bool()),(Some("response_too_large"),Some(true)));struct Broken;impl Write for Broken{fn write(&mut self,_:&[u8])->std::io::Result<usize>{Err(std::io::Error::other("no"))}fn flush(&mut self)->std::io::Result<()>{Ok(())}}assert_eq!(serve(&fake(),format!("{}\n",request("a")).as_bytes(),Broken),1)}
#[test]fn conservative_fallback_truth_is_correlated_and_bounded(){use boxology_contract::{OpaquePayload,OpaqueTree};fn result()->contract::RunTurnResult{contract::RunTurnResult{answer:"ok".into(),tool_name:None,tool_call_id:None,tool_output_json:None,usage:contract::TurnUsage{input_tokens:0,output_tokens:0,total_tokens:0}}}let outcomes=[contract::RunTurnOutcome{result:None,failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Tool,code:"large".into(),message:"x".repeat(MAX_LINE),retryable:false,side_effect_possible:true})},contract::RunTurnOutcome{result:None,failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Unknown{tag:"Future".into(),payload:OpaquePayload::new(OpaqueTree::Null)},code:"future".into(),message:"future".into(),retryable:false,side_effect_possible:true})},contract::RunTurnOutcome{result:Some(result()),failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Input,code:"both".into(),message:"both".into(),retryable:false,side_effect_possible:false})},contract::RunTurnOutcome{result:None,failure:None}];for(id,outcome)in["large","unknown","both","neither"].into_iter().zip(outcomes){let h=AgentLoopFake::new().with_run_turn(move|_,_|{let value=outcome.clone();async move{Ok(value)}}).handle();let mut bytes=vec![];assert_eq!(serve(&h,format!("{}\n",request(id)).as_bytes(),&mut bytes),0);assert!(bytes.len()<=MAX_LINE+1);let v:Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(v["id"],id);assert_eq!(v["error"]["side_effect_possible"],true);assert_eq!(v["error"]["code"],if id=="large"{"response_too_large"}else{"internal"})}}
#[test]fn compact_duplicate_unknown_and_negative_envelopes_are_correlated(){let input=format!("{}\n{}\n{}\n{}\n{}\n[]\n{{\"schema\":\"bad\",\"id\":\"schema\",\"method\":\"compact\",\"params\":{{}}}}\n{{\"schema\":\"{SCHEMA}\",\"id\":\"bad id\",\"method\":\"compact\",\"params\":{{}}}}\n{{\"schema\":\"{SCHEMA}\",\"id\":\"extra\",\"method\":\"compact\",\"params\":{{}},\"extra\":1}}\n",compact("c"),compact("c"),r#"{"schema":"boxology.agent-harness@1","id":"unknown","method":"future","params":{}}"#,compact("unknown"),r#"{"schema":"boxology.agent-harness@1","id":"params","method":"compact","params":{"extra":1}}"#);let(c,out)=run(input.as_bytes());assert_eq!(c,0);let v:Vec<Value>=out.lines().map(|line|serde_json::from_str(line).unwrap()).collect();assert_eq!(v[0]["result"],json!({"event_id":"checkpoint.c","sequence":7,"appended":true}));assert_eq!(v[1]["error"]["code"],"duplicate_id");assert_eq!(v[2]["error"]["code"],"unknown_method");assert_eq!(v[3]["error"]["code"],"duplicate_id");assert_eq!(v[4]["error"]["code"],"invalid_params");assert!(v[5]["id"].is_null());assert_eq!(v[6]["id"],"schema");assert!(v[7]["id"].is_null());assert_eq!(v[8]["id"],"extra")}
#[test]fn record_cap_is_correlated_and_sequential(){let calls=Arc::new(std::sync::atomic::AtomicUsize::new(0));let seen=calls.clone();let h=AgentLoopFake::new().with_run_turn(move|_,_|{seen.fetch_add(1,Ordering::SeqCst);async{Ok(contract::RunTurnOutcome{result:Some(contract::RunTurnResult{answer:"ok".into(),tool_name:None,tool_call_id:None,tool_output_json:None,usage:contract::TurnUsage{input_tokens:0,output_tokens:0,total_tokens:0}}),failure:None})}}).handle();let mut input=String::new();for n in 0..MAX_RECORDS{input.push_str(&request(&format!("r{n}")));input.push('\n')}input.push_str(&request("limit"));input.push('\n');let mut out=vec![];assert_eq!(serve(&h,input.as_bytes(),&mut out),1);let lines:Vec<Value>=String::from_utf8(out).unwrap().lines().map(|v|serde_json::from_str(v).unwrap()).collect();assert_eq!(calls.load(Ordering::SeqCst),MAX_RECORDS);assert_eq!(lines.len(),MAX_RECORDS+1);assert_eq!((lines.last().unwrap()["id"].as_str(),lines.last().unwrap()["error"]["code"].as_str()),(Some("limit"),Some("request_limit")));assert!(lines[..MAX_RECORDS].iter().enumerate().all(|(n,v)|v["id"]==format!("r{n}")))}
#[test]fn active_cancellation_responds_then_stops_and_idle_stop_is_clean(){let control=Arc::new(Control::default());let entered=Arc::new(std::sync::Barrier::new(2));let gate=entered.clone();let h=AgentLoopFake::new().with_run_turn(move|context,_|{let gate=gate.clone();async move{gate.wait();context.cancellation().cancelled().await;Ok(contract::RunTurnOutcome{result:None,failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Cancelled,code:"cancelled".into(),message:"cancelled".into(),retryable:false,side_effect_possible:true})})}}).handle();let input=format!("{}\n{}\n",request("active"),request("later"));let worker=std::thread::spawn({let control=control.clone();move||{let mut out=vec![];let code=serve_controlled(&h,input.as_bytes(),&mut out,control);(code,out)}});entered.wait();control.stop();let(code,out)=worker.join().unwrap();assert_eq!(code,0);let lines:Vec<Value>=String::from_utf8(out).unwrap().lines().map(|v|serde_json::from_str(v).unwrap()).collect();assert_eq!(lines.len(),1);assert_eq!((lines[0]["id"].as_str(),lines[0]["error"]["code"].as_str(),lines[0]["error"]["side_effect_possible"].as_bool()),(Some("active"),Some("cancelled"),Some(true)));let idle=Arc::new(Control::default());idle.stop();let mut out=vec![];assert_eq!(serve_controlled(&fake(),b"garbage\n".as_slice(),&mut out,idle),0);assert!(out.is_empty())}
#[test]fn stop_begin_boundary_always_cancels(){let control=Arc::new(Control::default());assert!(control.stop());let token=control.begin();assert!(token.is_cancelled());assert!(!control.stop());control.end();assert!(control.stop());let control=Arc::new(Control::default());let barrier=Arc::new(std::sync::Barrier::new(2));let worker=std::thread::spawn({let control=control.clone();let barrier=barrier.clone();move||{barrier.wait();control.begin()}});barrier.wait();control.stop();assert!(worker.join().unwrap().is_cancelled())}
#[test]fn deadline_failure_is_typed_and_correlated(){let h=AgentLoopFake::new().with_run_turn(|context,_|async move{assert!(context.deadline().is_some());Ok(contract::RunTurnOutcome{result:None,failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Deadline,code:"deadline_exceeded".into(),message:"deadline".into(),retryable:false,side_effect_possible:false})})}).handle();let mut out=vec![];assert_eq!(serve(&h,format!("{}\n",request("deadline")).as_bytes(),&mut out),0);let v:Value=serde_json::from_slice(&out).unwrap();assert_eq!((v["id"].as_str(),v["error"]["class"].as_str(),v["error"]["code"].as_str(),v["error"]["side_effect_possible"].as_bool()),(Some("deadline"),Some("deadline"),Some("deadline_exceeded"),Some(false)))}
#[test]fn compact_failure_projection_preserves_truth(){let h=AgentLoopFake::new().with_compact(|_,_|async{Ok(contract::CompactOutcome{result:None,failure:Some(contract::TurnFailure{class:contract::TurnFailureClass::Session,code:"conflict".into(),message:"conflict".into(),retryable:true,side_effect_possible:true})})}).handle();let mut out=vec![];assert_eq!(serve(&h,format!("{}\n",compact("compact-fail")).as_bytes(),&mut out),0);let v:Value=serde_json::from_slice(&out).unwrap();assert_eq!(v["error"],json!({"class":"session","code":"conflict","message":"conflict","retryable":true,"side_effect_possible":true}))}
#[test]fn cli_matrix(){let cwd=std::fs::canonicalize(".").unwrap();assert!(args2(&["--root".into(),cwd.clone().into_os_string(),"--state-dir".into(),cwd.into_os_string()]).is_some());let base=std::env::temp_dir().join(format!("harness-paths-{}",std::process::id()));let _=std::fs::remove_dir_all(&base);std::fs::create_dir(&base).unwrap();let file=base.join("file");std::fs::write(&file,b"x").unwrap();let link=base.with_extension("link");let _=std::fs::remove_file(&link);std::os::unix::fs::symlink(&base,&link).unwrap();for args in[vec!["--root".into(),"relative".into(),"--state-dir".into(),base.clone().into_os_string()],vec!["--root".into(),file.into_os_string(),"--state-dir".into(),base.clone().into_os_string()],vec!["--root".into(),link.into_os_string(),"--state-dir".into(),base.clone().into_os_string()],vec!["--root".into(),base.clone().into_os_string(),"--root".into(),base.clone().into_os_string()],vec!["--wat".into(),base.clone().into_os_string(),"--state-dir".into(),base.clone().into_os_string()]]{assert!(args2(&args).is_none())}let mut err=vec![];assert_eq!(main_entry(vec![],b"".as_slice(),vec![],&mut err),2);assert_eq!(err,b"invalid arguments\n");std::fs::remove_dir_all(base).unwrap()}}
