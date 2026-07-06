"""Local checks for no-SSH integration helper behavior."""

import asyncio

import pytest

from helpers.docker_control import (
    FaultInjectionUnavailable,
    start_warp_container,
    stop_warp_container,
)


@pytest.mark.parametrize("helper", [stop_warp_container, start_warp_container])
def test_warp_fault_injection_requires_safe_control_surface(helper):
    with pytest.raises(FaultInjectionUnavailable, match="direct SSH"):
        asyncio.run(helper("100.64.0.2"))
