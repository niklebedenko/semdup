<?php
// Derived from guzzle (src/RedirectMiddleware.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.
// Spec: Given a request, its options, and a redirect response, produce the
// follow-up request. 303 responses (and 301/302 unless strict RFC mode is
// on) downgrade non-safe methods to GET and drop the body. The target URI
// comes from the response's Location, restricted to the allowed protocols,
// with optional IDN normalisation. A Referer header (with userinfo removed)
// is set when enabled and the scheme is unchanged, otherwise removed.
// Authorization and Cookie headers are removed on cross-origin hops. The
// request body is rewound before reuse.

final class FollowUpRequests
{
    public static function fromRedirect(RequestInterface $request, array $config, ResponseInterface $reply): RequestInterface
    {
        $edits = [];

        $strict = $config['allow_redirects']['strict'];
        $code = $reply->getStatusCode();
        if ($code == 303 || (!$strict && $code >= 300 && $code <= 302)) {
            $method = $request->getMethod();
            $safe = in_array($method, ['GET', 'HEAD', 'OPTIONS'], true);
            $edits['method'] = $safe ? $method : 'GET';
            $edits['body'] = '';
        }

        $where = self::redirectUri($request, $reply, $config['allow_redirects']['protocols']);
        $idnMode = $config['idn_conversion'] ?? false;
        if ($idnMode !== false) {
            $where = Utils::idnUriConvert($where, $idnMode === true ? \IDNA_DEFAULT : $idnMode);
        }
        $edits['uri'] = $where;

        $refererWanted = $config['allow_redirects']['referer']
            && $where->getScheme() === $request->getUri()->getScheme();
        if ($refererWanted) {
            $edits['set_headers']['Referer'] = (string) $request->getUri()->withUserInfo('');
        } else {
            $edits['remove_headers'][] = 'Referer';
        }

        if (Psr7\UriComparator::isCrossOrigin($request->getUri(), $where)) {
            $edits['remove_headers'][] = 'Authorization';
            $edits['remove_headers'][] = 'Cookie';
        }

        Psr7\Message::rewindBody($request);

        return Psr7\Utils::modifyRequest($request, $edits);
    }
}
