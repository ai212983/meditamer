use super::*;

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wdevProcessRxSucDataAll(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_wdevProcessRxSucDataAll,
        &WDEV_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppProcessRxPktHdr(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppProcessRxPktHdr,
        &PPHDR_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxPkt(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_ppRxPkt, &PPRX_RING, a2, a3, a4, a5, a6, a7)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxProtoProc(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppRxProtoProc,
        &PPRX_PROTO_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxFragmentProc(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppRxFragmentProc,
        &PPRX_FRAG_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppEnqueueRxq(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_ppEnqueueRxq, &PPENQ_RING, a2, a3, a4, a5, a6, a7)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppDequeueRxq_Locked(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppDequeueRxq_Locked,
        &PPDEQ_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_sta_input(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_sta_input, &STA_INPUT_RING, a2, a3, a4, a5, a6, a7)
}
