{% autoescape 'html' %}{% if alert.hasAlert %}
      <div class="flash {{ alert.colour | default('info') }}" role="status">
        <span class="icon">{% if alert.colour == 'success' %}&#10003;{% elseif alert.colour == 'danger' %}!{% else %}{{ locale.hk_i|raw }}{% endif %}</span>
        <span class="msg">{{ alert.message }}</span>
        <button class="x" type="button" onclick="this.parentNode.remove()" aria-label="{{ locale.hk_cerrar|raw }}">&times;</button>
      </div>
{% endif %}
{% endautoescape %}
