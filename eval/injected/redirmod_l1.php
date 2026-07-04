<?php
// Derived from guzzle (src/RedirectMiddleware.php)
// at d281ed313b989f213357e3be1a179f02196ac99b, MIT licensed.
// Planted-clone eval asset for semdup; not production code.

final class HopRewriter
{
    public function rebuildForRedirect(RequestInterface $request, array $options, ResponseInterface $response): RequestInterface
    {
        // Request rewrites to apply.
        $changes = [];
        $schemes = $options['allow_redirects']['protocols'];

        // Downgrade to a GET when this is an entity-enclosing request and we
        // are not forcing RFC compliance, mirroring what every browser does.
        $code = $response->getStatusCode();
        if ($code == 303
            || ($code <= 302 && !$options['allow_redirects']['strict'])
        ) {
            $idempotent = ['GET', 'HEAD', 'OPTIONS'];
            $method = $request->getMethod();

            $changes['method'] = in_array($method, $idempotent) ? $method : 'GET';
            $changes['body'] = '';
        }

        $target = self::redirectUri($request, $response, $schemes);
        if (isset($options['idn_conversion']) && ($options['idn_conversion'] !== false)) {
            $idnFlags = ($options['idn_conversion'] === true) ? \IDNA_DEFAULT : $options['idn_conversion'];
            $target = Utils::idnUriConvert($target, $idnFlags);
        }

        $changes['uri'] = $target;
        Psr7\Message::rewindBody($request);

        // Only send a Referer when asked to, and never when the redirect
        // drops from https to http.
        if ($options['allow_redirects']['referer']
            && $changes['uri']->getScheme() === $request->getUri()->getScheme()
        ) {
            $referer = $request->getUri()->withUserInfo('');
            $changes['set_headers']['Referer'] = (string) $referer;
        } else {
            $changes['remove_headers'][] = 'Referer';
        }

        // Strip credentials when the redirect crosses origins.
        if (Psr7\UriComparator::isCrossOrigin($request->getUri(), $changes['uri'])) {
            $changes['remove_headers'][] = 'Authorization';
            $changes['remove_headers'][] = 'Cookie';
        }

        return Psr7\Utils::modifyRequest($request, $changes);
    }
}
