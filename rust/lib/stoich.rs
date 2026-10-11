//! In-process solver.
//!
//! Java loads this library and calls `solve` with one protobuf `SolveRequest`, getting back one
//! protobuf `SolveResponse`. A panic while solving is caught and returned as `REJECTED`; a panic
//! that escapes, or a native fault, takes the JVM down with it. That is the same outcome as Java
//! itself faulting, so there is no separate process to keep alive.

use jni::objects::{JByteArray, JClass};
use jni::sys::jbyteArray;
use jni::JNIEnv;
use prost::Message;
use std::panic::{catch_unwind, AssertUnwindSafe};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/stoich.rs"));
}

use proto::{Interval, SolveResponse, StartupCandidate, Status, Step};
use stoich_kernel::{SolveRequest, Status as KernelStatus, UnsignedAmount};

const MAX_REQUEST: usize = 64 * 1024 * 1024;

/// `byte[] solve(byte[] request)`. The request and response are protobuf `SolveRequest` and
/// `SolveResponse`, with no length prefix: the array length is the message length.
#[no_mangle]
pub extern "system" fn Java_y271727uy_stoich_internal_NativeSolver_solve0<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    request: JByteArray<'local>,
) -> jbyteArray {
    let response = match catch_unwind(AssertUnwindSafe(|| respond(&mut env, &request))) {
        Ok(response) => response,
        Err(_) => rejected(0, "the solver panicked on this request"),
    };
    let encoded = response.encode_to_vec();
    let Ok(array) = env.byte_array_from_slice(&encoded) else {
        return std::ptr::null_mut();
    };
    array.into_raw()
}

fn respond(env: &mut JNIEnv, request: &JByteArray) -> SolveResponse {
    let bytes = match env.convert_byte_array(request) {
        Ok(bytes) => bytes,
        Err(_) => return rejected(0, "the request bytes could not be read"),
    };
    if bytes.len() > MAX_REQUEST {
        return rejected(0, format!("refused a {} byte request", bytes.len()));
    }
    let request = match proto::SolveRequest::decode(&bytes[..]) {
        Ok(request) => request,
        Err(error) => return rejected(0, format!("malformed request: {error}")),
    };
    answer(request)
}

fn answer(request: proto::SolveRequest) -> SolveResponse {
    match decode_request(&request) {
        Ok(decoded) => match stoich_kernel::solve(&decoded) {
            Ok(result) => encode_result(request.request_id, result),
            Err(detail) => rejected(request.request_id, detail),
        },
        Err(detail) => rejected(request.request_id, detail),
    }
}

fn decode_request(request: &proto::SolveRequest) -> Result<SolveRequest, String> {
    let recipes = request.recipes as usize;
    let items = request.items as usize;
    if request.column_start.len() != recipes + 1 {
        return Err("column starts must contain one more entry than the recipe count".into());
    }
    let entries = request.item_index.len();
    if request.consumed.len() != entries || request.produced.len() != entries {
        return Err("consumed and produced arrays must match the sparse index".into());
    }
    if request.stock.len() != items || request.target.len() != items {
        return Err("stock and target must have one entry per item".into());
    }
    let mut consumed = vec![vec![0i64; items]; recipes];
    let mut produced = vec![vec![0i64; items]; recipes];
    for recipe in 0..recipes {
        let start = request.column_start[recipe] as usize;
        let end = request.column_start[recipe + 1] as usize;
        if start > end || end > entries {
            return Err(format!("recipe {recipe} has a column range outside the sparse arrays"));
        }
        for cursor in start..end {
            let item = request.item_index[cursor] as usize;
            if item >= items {
                return Err(format!("sparse entry references item {item}"));
            }
            consumed[recipe][item] = request.consumed[cursor];
            produced[recipe][item] = request.produced[cursor];
        }
    }
    let importable = (0..items).map(|item| bit(request.importable.as_slice(), item)).collect();
    let candidate = if request.has_candidate {
        if request.candidate.len() != recipes {
            return Err("candidate firing vector has a different recipe count".into());
        }
        Some(request.candidate.clone())
    } else {
        None
    };
    let order = if request.has_order {
        if request.order_recipe.len() != request.order_count.len() {
            return Err("fixed order recipes and counts have different lengths".into());
        }
        Some(
            request
                .order_recipe
                .iter()
                .zip(&request.order_count)
                .map(|(&recipe, &count)| (recipe as usize, count))
                .collect(),
        )
    } else {
        None
    };
    Ok(SolveRequest {
        consumed,
        produced,
        stock: request.stock.clone(),
        target: request.target.clone(),
        importable,
        candidate,
        order,
        target_item: request.has_target_item.then_some(request.target_item as usize),
        max_states: request.max_states,
        skip_equation: false,
    })
}

fn encode_result(request_id: u64, result: stoich_kernel::SolveResult) -> SolveResponse {
    let status = match result.status {
        KernelStatus::Success => Status::Success,
        KernelStatus::InsufficientSeed => Status::InsufficientSeed,
        KernelStatus::Infeasible => Status::Infeasible,
        KernelStatus::BudgetExhausted => Status::BudgetExhausted,
        KernelStatus::Unrepresentable => Status::Unrepresentable,
        KernelStatus::NotClosed => Status::NotClosed,
    };
    let amount = |value: &UnsignedAmount| value.to_i64().unwrap_or(i64::MAX);
    SolveResponse {
        request_id,
        status: status as i32,
        firings: result.firings.iter().map(amount).collect(),
        seed: result.seed.iter().filter_map(|value| value.to_i64()).collect(),
        external: result.external.iter().filter_map(|value| value.to_i64()).collect(),
        shortfall: result.shortfall.iter().filter_map(|value| value.to_i64()).collect(),
        startup: result
            .startup
            .iter()
            .map(|candidate| StartupCandidate { missing: candidate.iter().filter_map(|value| value.to_i64()).collect() })
            .collect(),
        steps: result
            .steps
            .iter()
            .map(|step| Step {
                recipe: step.recipe as u32,
                count: amount(&step.count),
                repeat_width: step.repeat_width,
                repetitions: amount(&step.repetitions),
            })
            .collect(),
        intervals: result
            .intervals
            .iter()
            .map(|interval| Interval {
                lower: amount(&interval.lower),
                has_upper: interval.upper.is_some(),
                upper: interval.upper.as_ref().map(amount).unwrap_or(0),
            })
            .collect(),
        states: result.states,
        detail: String::new(),
    }
}

fn rejected(request_id: u64, detail: impl Into<String>) -> SolveResponse {
    SolveResponse { request_id, status: Status::Rejected as i32, detail: detail.into(), ..SolveResponse::default() }
}

fn bit(bytes: &[u8], index: usize) -> bool {
    bytes.get(index / 8).is_some_and(|byte| byte & (1 << (index % 8)) != 0)
}
