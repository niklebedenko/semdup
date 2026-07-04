<?php
// Derived from guzzle (src/Utils.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Decide whether the proxy should be skipped for a host. The host may
// carry a ":port" suffix, which is ignored; an empty host is an error. Each
// rule in the list either matches every host ("*"), matches the host
// exactly, or matches subdomains when treated as a domain suffix (a rule
// "foo.com" or ".foo.com" matches "a.foo.com" but not "afoo.com"). Blank
// rules are skipped. True when any rule matches.

final class BypassPolicy
{
    public static function shouldBypass(string $hostname, array $rules): bool
    {
        if ($hostname === '') {
            throw new \InvalidArgumentException('host must be non-empty');
        }

        $bare = \explode(':', $hostname)[0];

        foreach ($rules as $rule) {
            if (!$rule) {
                continue;
            }
            if ($rule === '*' || $rule === $bare) {
                return true;
            }
            $suffix = '.'.\ltrim($rule, '.');
            if (\strlen($bare) >= \strlen($suffix)
                && \substr_compare($bare, $suffix, -\strlen($suffix)) === 0
            ) {
                return true;
            }
        }

        return false;
    }
}
