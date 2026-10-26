#!/usr/bin/env python3
"""Pure synthetic Target transcript controls, not browser/native acceptance."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
# Normal non-main import evaluates definitions only. Never call main,
# load_common, source_bytes, process control, display or network helpers.
spec = importlib.util.spec_from_file_location(
    "target_lifecycle_guards", Path(__file__).with_name("target-lifecycle-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class Synthetic:
    """Hand-authored command/phase arguments for the pure mutation checker."""
    def __init__(self):
        self.records, self.intervals = [], []
        self.sent, self.replies, self.positions = {}, {}, {}
        self.next_id = {"flattened": 1, "direct-A": 1, "direct-B": 1}
        self.targets = {"A": "page-1", "B": "page-4"}
        self.sessions = {"A": "session-1", "B": "session-2"}
        self.contexts = {"A": {"id": 7}, "B": {"id": 8}}
        self.negative_keys, self.activation_keys = [], []
        self.phase("context-A")
        self.create_key = self.command("Target.createTarget", {"url": "about:blank"},
                                       result={"targetId": "page-4"})
        self.phase("target-created", target="page-4", url="about:blank", native_tab=4)
        root = {"nodeId": 100001, "backendNodeId": 100001, "nodeType": 9,
                "nodeName": "#document", "localName": "", "nodeValue": "",
                "childNodeCount": 0, "children": [], "documentURL": "about:blank",
                "baseURL": "about:blank"}
        self.dom_key = self.command("DOM.getDocument", {"depth": -1},
                                    route="direct-B", result={"root": root})
        self.disable_key = self.command("DOM.disable", {}, route="direct-B", result={})
        self.phase("blank-B", target="page-4", root=copy.deepcopy(root))
        for label, method, params, code in (
            ("create-missing-url", "Target.createTarget", {}, -32602),
            ("create-nonstring-url", "Target.createTarget", {"url": False}, -32602),
            ("create-file-url", "Target.createTarget", {"url": "file:///tmp/not-admitted"}, -32602),
            ("create-extra-background", "Target.createTarget", {"url": "about:blank", "background": False}, -32602),
            ("create-foreign-context", "Target.createTarget", {"url": "about:blank", "browserContextId": "foreign"}, -32602),
            ("close-missing-target", "Target.closeTarget", {}, -32602),
            ("close-nonstring-target", "Target.closeTarget", {"targetId": 7}, -32602),
            ("activate-missing-target", "Target.activateTarget", {}, -32602),
            ("activate-extra-option", "Target.activateTarget", {"targetId": "page-4", "extra": True}, -32602),
            ("close-unknown-target", "Target.closeTarget", {"targetId": "page-18446744073709551615"}, -32000),
            ("activate-unknown-target", "Target.activateTarget", {"targetId": "page-18446744073709551615"}, -32000),
        ):
            self.negative(label, method, params, code)
        for suffix, route, session in (("direct", "direct-A", None),
                                        ("flattened", "flattened", "session-1")):
            for name, method in (("create", "Target.createTarget"),
                                 ("activate", "Target.activateTarget"),
                                 ("close", "Target.closeTarget")):
                params = {"url": "about:blank"} if name == "create" else {"targetId": "page-4"}
                self.negative(name + "-" + suffix + "-route", method, params, -32602,
                              route=route, session=session)
        self.phase("context-B")
        self.phase("roots")
        self.activation("A-initial", "A", 100)
        self.activation("B-initial", "B", 100)
        self.phase("native-drag")
        self.activation("A-detached", "A", 100)
        self.phase("paint", label="A-detached", window=100)
        self.activation("B-detached", "B", 200)
        self.phase("paint", label="B-detached", window=200)
        self.phase("native-drag")
        self.activation("B-redocked", "B", 100)
        self.phase("paint", label="B-redocked", window=100)
        events = [
            {"method": "Runtime.executionContextDestroyed", "sessionId": "session-2",
             "params": {"executionContextId": 8, "executionContextUniqueId": "mg-context-8"}},
            {"method": "Target.detachedFromTarget", "params": {"sessionId": "session-2"}},
        ]
        self.close_key = self.command("Target.closeTarget", {"targetId": "page-4"},
                                      result={"success": True}, events=events)
        self.phase("direct-closed")
        self.phase("closed-endpoint")
        self.negative("close-stale-target", "Target.closeTarget", {"targetId": "page-4"}, -32000)
        self.negative("activate-stale-target", "Target.activateTarget", {"targetId": "page-4"}, -32000)
        self.phase("paint", label="A-survivor", window=100)
        self.phase("closed-B")
        self.negative("close-last-target", "Target.closeTarget", {"targetId": "page-1"}, -32000)
        self.phase("target-lifecycle-proof")

    def phase(self, name, **fields):
        self.records.append({"phase": name, **fields})

    def command(self, method, params, *, route="flattened", session=None,
                result=None, error=None, events=()):
        number = self.next_id[route]
        self.next_id[route] += 1
        key = (route, number)
        request = {"id": number, "method": method, "params": copy.deepcopy(params)}
        response = {"id": number}
        response["error" if error is not None else "result"] = copy.deepcopy(error if error is not None else result)
        if session is not None:
            request["sessionId"] = response["sessionId"] = session
        self.sent[key], self.replies[key] = request, response
        start = len(self.records)
        self.records.append({"protocol": "send", "route": route, "message": request})
        self.records.extend({"protocol": "receive", "route": route, "message": copy.deepcopy(event)}
                            for event in events)
        self.records.append({"protocol": "receive", "route": route, "message": response})
        self.positions[key] = [start, len(self.records) - 1]
        return key

    def negative(self, label, method, params, code, *, route="flattened", session=None):
        previous = self.intervals[-1]["record_position"] if self.intervals else -1
        key = self.command(method, params, route=route, session=session,
                           error={"code": code, "message": "synthetic explicit rejection"})
        row = {"phase": "log-interval", "kind": "quiet", "realm": None, "sequence": None,
               "label": label, "command": {"route": route, "id": key[1]},
               "previous_interval_position": previous, "record_position": len(self.records)}
        self.intervals.append(row)
        self.records.append(row)
        self.negative_keys.append(key)

    def activation(self, label, role, window):
        key = self.command("Target.activateTarget", {"targetId": self.targets[role]}, result={})
        self.activation_keys.append(key)
        self.phase("target-activated", label=label, target=self.targets[role], window=window,
                   actual_input_focus=window, title="Runtime move " + role)

    def check(self):
        return smoke.check_mutations(self.records,
            [row for row in self.records if row.get("phase")], self.intervals,
            self.sent, self.replies, self.positions, self.contexts, self.sessions, self.targets)

    def phase_named(self, name):
        return next(row for row in self.records if row.get("phase") == name)

    def reindex_protocol(self):
        """Keep actual response indices consistent when testing reordered wire rows."""
        self.positions = {key: [] for key in self.sent}
        for index, row in enumerate(self.records):
            if row.get("protocol") in ("send", "receive") and "id" in row.get("message", {}):
                self.positions[(row["route"], row["message"]["id"])].append(index)


class MutationControls(unittest.TestCase):
    def reject(self, fixture, message):
        with self.assertRaisesRegex(RuntimeError, message):
            fixture.check()

    def test_declared_synthetic_positive_has_exact_seven_and_twenty(self):
        result = Synthetic().check()
        self.assertEqual(len(result["rejected_keys"]), 20)
        self.assertEqual(result["evidence"]["successful_commands"], 7)
        self.assertEqual(result["evidence"]["negative_commands"], 20)
        self.assertTrue(result["evidence"]["last_target_rejected"])

    def test_missing_or_forged_create_result_is_rejected(self):
        for change in ("missing", "forged", "extra-option"):
            fixture = Synthetic()
            if change == "missing":
                fixture.replies[fixture.create_key].pop("result")
                fixture.replies[fixture.create_key]["error"] = {"code": -32601}
                expected = "successful mutation count"
            elif change == "forged":
                fixture.replies[fixture.create_key]["result"]["targetId"] = "page-9"
                expected = "Create must return"
            else:
                fixture.sent[fixture.create_key]["params"]["background"] = False
                expected = "Create must return"
            with self.subTest(change=change):
                self.reject(fixture, expected)

    def test_create_ack_must_precede_created_phase_and_blank_inspection(self):
        fixture = Synthetic()
        reply = fixture.positions[fixture.create_key][1]
        phase = fixture.records.index(fixture.phase_named("target-created"))
        fixture.records[reply], fixture.records[phase] = fixture.records[phase], fixture.records[reply]
        fixture.reindex_protocol()
        self.reject(fixture, "Create must return")

    def test_welcome_text_or_child_nodes_cannot_masquerade_as_blank(self):
        for change in ("welcome", "element", "wrong-url"):
            fixture = Synthetic()
            root = fixture.phase_named("blank-B")["root"]
            if change == "welcome": root["nodeValue"] = "mgbrowser"
            elif change == "element":
                root["childNodeCount"], root["children"] = 1, [{"nodeName": "H1"}]
            else: root["documentURL"] = "https://mgbrowser.org/"
            fixture.replies[fixture.dom_key]["result"] = {"root": copy.deepcopy(root)}
            with self.subTest(change=change):
                self.reject(fixture, "Blank target contains")

    def test_blank_dom_must_be_actual_direct_b_response_and_disabled(self):
        for change in ("foreign-session", "different-reply", "disable-failed"):
            fixture = Synthetic()
            if change == "foreign-session": fixture.sent[fixture.dom_key]["sessionId"] = "session-1"
            elif change == "different-reply": fixture.replies[fixture.dom_key]["result"]["root"]["nodeId"] += 1
            else: fixture.replies[fixture.disable_key]["result"] = {"not-disabled": True}
            with self.subTest(change=change):
                self.reject(fixture, "Blank DOM not bound|DOM disable not acknowledged")

    def test_successful_mutation_cannot_use_a_flattened_page_session(self):
        fixture = Synthetic()
        fixture.sent[fixture.create_key]["sessionId"] = "session-1"
        self.reject(fixture, "unauthorized route")

    def test_missing_activation_or_wrong_actual_focus_is_rejected(self):
        for change in ("missing", "focus", "target", "title"):
            fixture = Synthetic()
            row = fixture.phase_named("target-activated")
            if change == "missing": fixture.records.remove(row)
            elif change == "focus": row["actual_input_focus"] = 200
            elif change == "target": row["target"] = "page-4"
            else: row["title"] = "unrelated title"
            with self.subTest(change=change):
                self.reject(fixture, "phase order|activation/focus/title")

    def test_close_requires_exact_named_success(self):
        for change in ("wrong-target", "false", "extra"):
            fixture = Synthetic()
            if change == "wrong-target": fixture.sent[fixture.close_key]["params"]["targetId"] = "page-1"
            elif change == "false": fixture.replies[fixture.close_key]["result"] = {"success": False}
            else: fixture.replies[fixture.close_key]["result"] = {"success": True, "extra": 1}
            with self.subTest(change=change):
                self.reject(fixture, "Close result not bound")

    def test_teardown_after_success_reply_is_not_accepted(self):
        fixture = Synthetic()
        end = fixture.positions[fixture.close_key][1]
        fixture.records[end-1], fixture.records[end] = fixture.records[end], fixture.records[end-1]
        fixture.reindex_protocol()
        self.reject(fixture, "Close success preceded")

    def test_missing_wrong_session_or_reversed_teardown_is_rejected(self):
        for change in ("missing", "wrong-session", "reversed"):
            fixture = Synthetic()
            start, end = fixture.positions[fixture.close_key]
            if change == "missing": fixture.records.pop(start+1)
            elif change == "wrong-session": fixture.records[start+1]["message"]["sessionId"] = "session-1"
            else: fixture.records[start+1], fixture.records[end-1] = fixture.records[end-1], fixture.records[start+1]
            fixture.reindex_protocol()
            with self.subTest(change=change):
                self.reject(fixture, "Close success preceded")

    def test_missing_or_extra_negative_interval_is_rejected(self):
        for change in ("missing", "extra"):
            fixture = Synthetic()
            if change == "missing": fixture.intervals.pop()
            else: fixture.intervals.append(copy.deepcopy(fixture.intervals[-1]))
            with self.subTest(change=change):
                self.reject(fixture, "mutation rejection roster")

    def test_wrong_negative_code_route_or_options_are_rejected(self):
        for change in ("code", "route", "params"):
            fixture = Synthetic()
            key = fixture.negative_keys[0]
            if change == "code": fixture.replies[key]["error"]["code"] = -32601
            elif change == "route": fixture.sent[key]["sessionId"] = "session-2"
            else: fixture.sent[key]["params"] = {"url": "about:blank"}
            with self.subTest(change=change):
                self.reject(fixture, "Negative not bound")

    def test_unaccounted_success_or_rejection_cannot_hide_outside_intervals(self):
        for error in (None, {"code": -32000}):
            fixture = Synthetic()
            fixture.command("Target.activateTarget", {"targetId": "page-1"}, result={}, error=error)
            with self.subTest(error=error):
                self.reject(fixture, "successful mutation count|Unaccounted host mutation")

    def test_last_target_rejection_cannot_be_replaced_by_success(self):
        fixture = Synthetic()
        response = fixture.replies[fixture.negative_keys[-1]]
        response.pop("error")
        response["result"] = {"success": True}
        self.reject(fixture, "successful mutation count")


if __name__ == "__main__":
    unittest.main()
